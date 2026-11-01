//! The `witforge` command-line interface.
//!
//! ```text
//! witforge check <wit-dir> [--lock PATH] [--first | --against PATH] [--released PATH]
//! witforge lock  <wit-dir> [--lock PATH] [--first | --against PATH] [--released PATH]
//! witforge rust  <wit-root> --out DIR [--check] [--released PATH]
//! witforge proto <wit-root> --out DIR [--check] [--released PATH]
//! witforge glue  <wit-root> --out DIR --plain-root PATH --proto-root PATH [--check] [--released PATH]
//! ```

use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};

use witforge::emit;
use witforge::lock::{LockMode, WireLock};
use witforge::{
    ContractError, ensure_lockfile_not_released, ensure_not_released, is_contract_error, load,
    load_released_manifest, load_root_released_manifest, model, subset,
};

/// The witforge contract compiler.
#[derive(Debug, Parser)]
#[command(name = "witforge", version, about)]
struct Cli {
    /// Command to run.
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Check the subset and the wire lockfile without writing.
    Check(Options),
    /// Assign wire numbers to new members and reserve removed ones.
    Lock(Options),
    /// Generate plain Rust types and interface traits.
    Rust(RustOptions),
    /// Generate protobuf schemas.
    Proto(ProtoOptions),
    /// Generate protobuf glue conversions and function helpers.
    Glue(GlueOptions),
}

#[derive(Debug, Args)]
struct RustOptions {
    /// WIT root containing version directories.
    #[arg(value_name = "WIT_ROOT")]
    root: PathBuf,
    /// Output root for generated Rust files.
    #[arg(long, value_name = "DIR")]
    out: PathBuf,
    /// Check generated output without writing.
    #[arg(long)]
    check: bool,
    /// Released-version manifest to validate; defaults to `<WIT_ROOT>/released`.
    #[arg(long, value_name = "PATH")]
    released: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct ProtoOptions {
    /// WIT root containing version directories.
    #[arg(value_name = "WIT_ROOT")]
    root: PathBuf,
    /// Output root for generated proto files.
    #[arg(long, value_name = "DIR")]
    out: PathBuf,
    /// Check generated output without writing.
    #[arg(long)]
    check: bool,
    /// Released-version manifest to validate; defaults to `<WIT_ROOT>/released`.
    #[arg(long, value_name = "PATH")]
    released: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct GlueOptions {
    /// WIT root containing version directories.
    #[arg(value_name = "WIT_ROOT")]
    root: PathBuf,
    /// Output root for generated glue files.
    #[arg(long, value_name = "DIR")]
    out: PathBuf,
    /// Rust module containing the generated plain version modules.
    #[arg(long, value_name = "PATH")]
    plain_root: String,
    /// Rust module containing the generated proto version modules.
    #[arg(long, value_name = "PATH")]
    proto_root: String,
    /// Check generated output without writing.
    #[arg(long)]
    check: bool,
    /// Released-version manifest to validate; defaults to `<WIT_ROOT>/released`.
    #[arg(long, value_name = "PATH")]
    released: Option<PathBuf>,
}

#[derive(Debug, Args)]
struct Options {
    /// WIT version directory.
    #[arg(value_name = "WIT_DIR")]
    dir: PathBuf,
    /// Lockfile path; defaults to `<WIT_DIR>/wire.lock`.
    #[arg(long, value_name = "PATH")]
    lock: Option<PathBuf>,
    /// Treat entries from this previous lockfile as released.
    #[arg(long, value_name = "PATH", conflicts_with = "first")]
    against: Option<PathBuf>,
    /// Treat every existing entry as a draft; for a version with no baseline.
    #[arg(long, conflicts_with = "against")]
    first: bool,
    /// Released-version manifest (`lock` refuses a listed directory); defaults to
    /// `<WIT_DIR>/../released`.
    #[arg(long, value_name = "PATH")]
    released: Option<PathBuf>,
}

/// Exit codes: `0` success; `1` a contract, lockfile, or generated-output violation; `2` a
/// usage, I/O, parse, released-directory, or internal error.
fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(code) => code,
        Err(err) => {
            eprintln!("witforge: {err:#}");
            ExitCode::from(if is_contract_error(&err) { 1 } else { 2 })
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode> {
    match cli.command {
        Command::Check(options) => check(&options),
        Command::Lock(options) => lock(&options),
        Command::Rust(options) => rust(&options),
        Command::Proto(options) => proto(&options),
        Command::Glue(options) => glue(&options),
    }
}

fn check(options: &Options) -> Result<ExitCode> {
    // `check` may verify a released directory; the manifest only has to be valid.
    load_released_manifest(&options.dir, options.released.as_deref())?;
    let contract = load(&options.dir)?;
    let violations = subset::check(&contract);
    if !violations.is_empty() {
        // The model of a contract outside the subset is not built, so every locked member
        // would look removed; report only the violations, as `lock` does.
        for violation in &violations {
            eprintln!("error: {violation}");
        }
        return Ok(ExitCode::from(1));
    }
    let members = model::members(&contract)?;
    let lock_path = options
        .lock
        .clone()
        .unwrap_or_else(|| options.dir.join("wire.lock"));
    let lock = read_lock(&lock_path, options)?;
    let previous = read_previous(options.against.as_deref())?;
    let mode = lock_mode(options, previous.as_ref());
    let drift = match lock.drift_with(&members, mode) {
        Ok(changes) => changes,
        Err(err) => {
            eprintln!("error: {err:#}");
            return Ok(ExitCode::from(1));
        }
    };
    report_check_changes(&drift, &lock_command(options));
    if drift.is_empty() {
        Ok(ExitCode::SUCCESS)
    } else {
        Ok(ExitCode::from(1))
    }
}

fn lock(options: &Options) -> Result<ExitCode> {
    ensure_not_released(&options.dir, options.released.as_deref())?;
    let contract = load(&options.dir)?;
    let violations = subset::check(&contract);
    for violation in &violations {
        eprintln!("error: {violation}");
    }
    if !violations.is_empty() {
        bail!(ContractError::new(format!(
            "{} subset violation(s); the lockfile was not updated",
            violations.len()
        )));
    }
    let members = model::members(&contract)?;
    let lock_path = options
        .lock
        .clone()
        .unwrap_or_else(|| options.dir.join("wire.lock"));
    ensure_lockfile_not_released(&lock_path, &options.dir, options.released.as_deref())?;
    let mut lock = read_lock(&lock_path, options)?;
    let previous = read_previous(options.against.as_deref())?;
    let mode = lock_mode(options, previous.as_ref());
    // Every refusal from reconciling is a lockfile violation, as `check` reports it.
    let changes = lock
        .reconcile_with(&members, mode)
        .map_err(|err| ContractError::from_error(&err))?;
    if !changes.reintroduced.is_empty() {
        for (key, number) in &changes.reintroduced {
            eprintln!("error: {}", reintroduced(key, *number));
        }
        bail!(ContractError::new(format!(
            "{} removed member(s) reintroduced; the lockfile was not updated",
            changes.reintroduced.len()
        )));
    }
    for (key, number) in &changes.dropped {
        println!("dropped  {key} = {number}");
    }
    for (key, _kind, _ty) in &changes.updated {
        println!("updated  {key}");
    }
    for (key, number) in &changes.reserved {
        println!("reserved {key} = {number}");
    }
    for (key, number) in &changes.added {
        println!("added    {key} = {number}");
    }
    lock.write_atomic(&lock_path)?;
    Ok(ExitCode::SUCCESS)
}

fn rust(options: &RustOptions) -> Result<ExitCode> {
    // Generation reproduces released versions without modifying them, so the manifest only has
    // to be valid. The emitter validates every version and lockfile before writing.
    load_root_released_manifest(&options.root, options.released.as_deref())?;
    if options.check {
        let messages = emit::rust::check(&options.root, &options.out)?;
        for message in &messages {
            eprintln!("error: {message}");
        }
        return if messages.is_empty() {
            Ok(ExitCode::SUCCESS)
        } else {
            Ok(ExitCode::from(1))
        };
    }
    emit::rust::generate(&options.root, &options.out)?;
    Ok(ExitCode::SUCCESS)
}

fn proto(options: &ProtoOptions) -> Result<ExitCode> {
    load_root_released_manifest(&options.root, options.released.as_deref())?;
    if options.check {
        let messages = emit::proto::check(&options.root, &options.out)?;
        for message in &messages {
            eprintln!("error: {message}");
        }
        return if messages.is_empty() {
            Ok(ExitCode::SUCCESS)
        } else {
            Ok(ExitCode::from(1))
        };
    }
    emit::proto::generate(&options.root, &options.out)?;
    Ok(ExitCode::SUCCESS)
}

fn glue(options: &GlueOptions) -> Result<ExitCode> {
    // Glue is current-schema output for the highest minor of each major; released plain modules
    // stay frozen. As for the other generators, the manifest only has to be valid.
    load_root_released_manifest(&options.root, options.released.as_deref())?;
    if options.check {
        let messages = emit::glue::check(
            &options.root,
            &options.out,
            &options.plain_root,
            &options.proto_root,
        )?;
        for message in &messages {
            eprintln!("error: {message}");
        }
        return if messages.is_empty() {
            Ok(ExitCode::SUCCESS)
        } else {
            Ok(ExitCode::from(1))
        };
    }
    emit::glue::generate(
        &options.root,
        &options.out,
        &options.plain_root,
        &options.proto_root,
    )?;
    Ok(ExitCode::SUCCESS)
}

fn lock_mode<'a>(options: &Options, previous: Option<&'a WireLock>) -> LockMode<'a> {
    if let Some(previous) = previous {
        LockMode::Against(previous)
    } else if options.first {
        LockMode::First
    } else {
        LockMode::Default
    }
}

fn read_previous(path: Option<&Path>) -> Result<Option<WireLock>> {
    match path {
        Some(path) => read_required_lock(path).map(Some),
        None => Ok(None),
    }
}

/// Read the current lockfile. Only `--first` treats a missing one as empty: in default mode a
/// fresh lockfile would renumber a new minor that should continue the previous one, and
/// `--against` develops a copy of the previous lockfile.
fn read_lock(path: &Path, options: &Options) -> Result<WireLock> {
    match fs::read_to_string(path) {
        Ok(text) => WireLock::parse(&text).with_context(|| format!("in {}", path.display())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            if options.first {
                return Ok(WireLock::default());
            }
            if let Some(previous) = &options.against {
                bail!(
                    "lockfile {} does not exist; a new minor starts as a copy of the previous \
                     minor's lockfile: copy {} to {} first, then rerun with `--against`",
                    path.display(),
                    previous.display(),
                    path.display()
                );
            }
            bail!(
                "lockfile {path} does not exist; for the first version of a package or of a new \
                 major, create it with `witforge lock {dir} --first`; for a new minor, copy the \
                 previous minor's wire.lock to {path} and run `witforge lock {dir} --against \
                 <previous minor's wire.lock>`",
                path = path.display(),
                dir = options.dir.display()
            );
        }
        Err(err) => Err(err).with_context(|| format!("failed to read {}", path.display())),
    }
}

fn read_required_lock(path: &Path) -> Result<WireLock> {
    let text =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    WireLock::parse(&text).with_context(|| format!("in {}", path.display()))
}

fn report_check_changes(changes: &witforge::lock::Changes, lock: &str) {
    for (key, _) in &changes.added {
        eprintln!("error: {key} has no wire number; run `{lock}`");
    }
    for (key, number) in &changes.reserved {
        eprintln!(
            "error: {key} = {number} is no longer in the contract; run `{lock}` to reserve it"
        );
    }
    for (key, number) in &changes.reintroduced {
        eprintln!("error: {}", reintroduced(key, *number));
    }
    for (key, number) in &changes.dropped {
        eprintln!("error: stale draft {key} = {number}; run `{lock}` to drop it");
    }
    for (key, _kind, _ty) in &changes.updated {
        eprintln!("error: draft {key} changed kind or type; run `{lock}` to update it");
    }
}

/// The `witforge lock` command that fixes what `check` reports, with the same mode and paths.
fn lock_command(options: &Options) -> String {
    let mut command = format!("witforge lock {}", options.dir.display());
    if let Some(lock) = &options.lock {
        let _ = write!(command, " --lock {}", lock.display());
    }
    if options.first {
        command.push_str(" --first");
    }
    if let Some(previous) = &options.against {
        let _ = write!(command, " --against {}", previous.display());
    }
    if let Some(released) = &options.released {
        let _ = write!(command, " --released {}", released.display());
    }
    command
}

fn reintroduced(key: &str, number: u32) -> String {
    format!(
        "{key} was removed and its name and number {number} stay reserved; give the member a new name"
    )
}

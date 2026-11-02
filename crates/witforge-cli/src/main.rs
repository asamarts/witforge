//! The `witforge` command-line interface.
//!
//! ```text
//! witforge check <wit-dir> [--lock PATH] [--first | --against PATH] [--released PATH]
//! witforge lock  <wit-dir> [--lock PATH] [--first | --against PATH] [--released PATH]
//! witforge rust  [<wit-root>] [--out DIR] [--config PATH] [--project NAME]
//!                [--check] [--released PATH]
//! witforge proto [<wit-root>] [--out DIR] [--config PATH] [--project NAME]
//!                [--check] [--released PATH]
//! witforge schema [<wit-root>] [--out DIR] [--config PATH] [--project NAME]
//!                [--check] [--released PATH]
//! witforge glue  [<wit-root>] [--out DIR] [--plain-root PATH] [--proto-root PATH]
//!                [--config PATH] [--project NAME] [--check] [--released PATH]
//! ```

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};
use serde::Deserialize;

use witforge::emit;
use witforge::lock::{LockMode, PackageName, WireLock};
use witforge::{
    ContractError, ensure_directory_version, ensure_lockfile_not_released, ensure_not_released,
    ensure_version_directory, is_contract_error, is_released, load, load_released_manifest,
    load_root_released_manifest, model, subset,
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
    /// Generate Draft 7 JSON Schema definitions.
    Schema(SchemaOptions),
    /// Generate protobuf glue conversions and function helpers.
    Glue(GlueOptions),
}

#[derive(Debug, Args)]
struct RustOptions {
    /// WIT root containing version directories.
    #[arg(value_name = "WIT_ROOT")]
    root: Option<PathBuf>,
    /// Output root for generated Rust files.
    #[arg(long, value_name = "DIR")]
    out: Option<PathBuf>,
    /// Check generated output without writing.
    #[arg(long)]
    check: bool,
    /// Released-version manifest to validate; defaults to `<WIT_ROOT>/released`.
    #[arg(long, value_name = "PATH")]
    released: Option<PathBuf>,
    /// Configuration file with a project root and emitter outputs.
    #[command(flatten)]
    config: ConfigArgs,
}

#[derive(Debug, Args)]
struct ProtoOptions {
    /// WIT root containing version directories.
    #[arg(value_name = "WIT_ROOT")]
    root: Option<PathBuf>,
    /// Output root for generated proto files.
    #[arg(long, value_name = "DIR")]
    out: Option<PathBuf>,
    /// Check generated output without writing.
    #[arg(long)]
    check: bool,
    /// Released-version manifest to validate; defaults to `<WIT_ROOT>/released`.
    #[arg(long, value_name = "PATH")]
    released: Option<PathBuf>,
    /// Configuration file with a project root and emitter outputs.
    #[command(flatten)]
    config: ConfigArgs,
}

#[derive(Debug, Args)]
struct SchemaOptions {
    /// WIT root containing version directories.
    #[arg(value_name = "WIT_ROOT")]
    root: Option<PathBuf>,
    /// Output root for generated JSON Schema files.
    #[arg(long, value_name = "DIR")]
    out: Option<PathBuf>,
    /// Check generated output without writing.
    #[arg(long)]
    check: bool,
    /// Released-version manifest to validate; defaults to `<WIT_ROOT>/released`.
    #[arg(long, value_name = "PATH")]
    released: Option<PathBuf>,
    /// Configuration file with a project root and emitter outputs.
    #[command(flatten)]
    config: ConfigArgs,
}

#[derive(Debug, Args)]
struct GlueOptions {
    /// WIT root containing version directories.
    #[arg(value_name = "WIT_ROOT")]
    root: Option<PathBuf>,
    /// Output root for generated glue files.
    #[arg(long, value_name = "DIR")]
    out: Option<PathBuf>,
    /// Rust module containing the generated plain version modules.
    #[arg(long, value_name = "PATH")]
    plain_root: Option<String>,
    /// Rust module containing the generated proto version modules.
    #[arg(long, value_name = "PATH")]
    proto_root: Option<String>,
    /// Check generated output without writing.
    #[arg(long)]
    check: bool,
    /// Released-version manifest to validate; defaults to `<WIT_ROOT>/released`.
    #[arg(long, value_name = "PATH")]
    released: Option<PathBuf>,
    /// Configuration file with a project root and emitter outputs.
    #[command(flatten)]
    config: ConfigArgs,
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

/// Configuration-file selection shared by the generator commands.
#[derive(Debug, Args)]
struct ConfigArgs {
    /// TOML configuration file; paths inside it are relative to the file.
    #[arg(long, value_name = "PATH")]
    config: Option<PathBuf>,
    /// Project name in the configuration file; required when it has more than one.
    #[arg(long, value_name = "NAME")]
    project: Option<String>,
}

/// The supported `witforge.toml` format.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct ConfigFile {
    /// Format version; currently `1`.
    version: u32,
    /// Named projects.
    project: BTreeMap<String, ProjectConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProjectConfig {
    /// WIT root, relative to the configuration file.
    root: PathBuf,
    /// Released-version manifest, relative to the configuration file.
    released: Option<PathBuf>,
    rust: Option<OutConfig>,
    proto: Option<OutConfig>,
    schema: Option<OutConfig>,
    glue: Option<GlueConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct OutConfig {
    out: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct GlueConfig {
    out: PathBuf,
    #[serde(rename = "plain-root")]
    plain_root: Option<String>,
    #[serde(rename = "proto-root")]
    proto_root: Option<String>,
}

#[derive(Debug, Clone, Copy)]
enum Emitter {
    Rust,
    Proto,
    Schema,
    Glue,
}

struct ResolvedPaths {
    root: PathBuf,
    out: PathBuf,
    released: Option<PathBuf>,
    plain_root: Option<String>,
    proto_root: Option<String>,
}

fn resolve_paths(
    emitter: Emitter,
    config_args: &ConfigArgs,
    root: Option<PathBuf>,
    out: Option<PathBuf>,
    released: Option<PathBuf>,
    plain_root: Option<String>,
    proto_root: Option<String>,
) -> Result<ResolvedPaths> {
    let Some(config_path) = &config_args.config else {
        if config_args.project.is_some() {
            bail!("`--project` requires `--config`");
        }
        let root = root.context("missing WIT_ROOT (or use --config)")?;
        let out = out.context("missing --out (or use --config)")?;
        if matches!(emitter, Emitter::Glue) && (plain_root.is_none() || proto_root.is_none()) {
            bail!("glue requires --plain-root and --proto-root (or use --config)");
        }
        return Ok(ResolvedPaths {
            root,
            out,
            released,
            plain_root,
            proto_root,
        });
    };

    let text = fs::read_to_string(config_path)
        .with_context(|| format!("failed to read configuration {}", config_path.display()))?;
    let config: ConfigFile = toml::from_str(&text)
        .with_context(|| format!("invalid configuration {}", config_path.display()))?;
    if config.version != 1 {
        bail!(
            "configuration {} has unsupported version {}; expected 1",
            config_path.display(),
            config.version
        );
    }
    if config.project.is_empty() {
        bail!("configuration {} has no projects", config_path.display());
    }
    let name = match &config_args.project {
        Some(name) => name.clone(),
        None if config.project.len() == 1 => config
            .project
            .keys()
            .next()
            .expect("checked one project")
            .clone(),
        None => bail!(
            "configuration {} has multiple projects; pass --project",
            config_path.display()
        ),
    };
    let project = config
        .project
        .get(&name)
        .with_context(|| format!("configuration has no project `{name}`"))?
        .clone();
    let base = config_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let from_config = |path: &Path| {
        if path.is_absolute() {
            path.to_path_buf()
        } else {
            base.join(path)
        }
    };
    let root = root.unwrap_or_else(|| from_config(&project.root));
    let out = out.or_else(|| match emitter {
        Emitter::Rust => project.rust.map(|config| from_config(&config.out)),
        Emitter::Proto => project.proto.map(|config| from_config(&config.out)),
        Emitter::Schema => project.schema.map(|config| from_config(&config.out)),
        Emitter::Glue => project.glue.as_ref().map(|config| from_config(&config.out)),
    });
    let out = out.context("missing --out (and no matching output section in the configuration)")?;
    let released = released.or_else(|| project.released.as_deref().map(from_config));
    let plain_root = plain_root.or_else(|| {
        project
            .glue
            .as_ref()
            .and_then(|config| config.plain_root.clone())
    });
    let proto_root = proto_root.or_else(|| {
        project
            .glue
            .as_ref()
            .and_then(|config| config.proto_root.clone())
    });
    if matches!(emitter, Emitter::Glue) && (plain_root.is_none() || proto_root.is_none()) {
        bail!("glue requires --plain-root and --proto-root (or a configuration section)");
    }
    Ok(ResolvedPaths {
        root,
        out,
        released,
        plain_root,
        proto_root,
    })
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
        Command::Schema(options) => schema(&options),
        Command::Glue(options) => glue(&options),
    }
}

fn check(options: &Options) -> Result<ExitCode> {
    ensure_version_directory(&options.dir)?;
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
    let model = model::build(&contract)?;
    ensure_directory_version(&options.dir, &model)?;
    let members = model.members()?;
    let lock_path = options
        .lock
        .clone()
        .unwrap_or_else(|| options.dir.join("wire.lock"));
    let (lock, text) = read_lock(&lock_path, options)?;
    let previous = read_previous(options.against.as_deref())?;
    let released = is_released(&options.dir, options.released.as_deref())?;
    let remedy = if released {
        Remedy::Released(options.dir.display().to_string())
    } else {
        Remedy::Lock(lock_command(options))
    };
    let package = package_name(&model);
    for lockfile in std::iter::once(&lock).chain(previous.as_ref()) {
        if let Err(err) = lockfile.check_package(&package) {
            eprintln!("error: {err:#}");
            return Ok(ExitCode::from(1));
        }
    }
    let mode = lock_mode(options, previous.as_ref());
    let drift = match lock.drift_with(&members, mode) {
        Ok(changes) => changes,
        Err(err) => {
            eprintln!("error: {err:#}");
            return Ok(ExitCode::from(1));
        }
    };
    // A lockfile written by hand, or edited by a tool that reorders or reformats it, is drift
    // too: `lock` would rewrite it, and only its canonical form is reviewed. CRLF line endings
    // are read as LF, since a checkout with `core.autocrlf` writes them. A released lockfile is
    // exempt: `lock` refuses to rewrite it, and the freeze check guards it as it was released.
    let canonical = released
        || text
            .as_deref()
            .is_none_or(|text| lock.is_canonical(&text.replace("\r\n", "\n")));
    if !canonical {
        eprintln!(
            "error: lockfile {} is not in the form `witforge lock` writes; {}",
            lock_path.display(),
            remedy.advice(" to rewrite it")
        );
    }
    report_check_changes(&drift, &remedy);
    if drift.is_empty() && canonical {
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
    let model = model::build(&contract)?;
    ensure_directory_version(&options.dir, &model)?;
    let members = model.members()?;
    let lock_path = options
        .lock
        .clone()
        .unwrap_or_else(|| options.dir.join("wire.lock"));
    ensure_lockfile_not_released(&lock_path, &options.dir, options.released.as_deref())?;
    let (mut lock, _) = read_lock(&lock_path, options)?;
    let previous = read_previous(options.against.as_deref())?;
    let package = package_name(&model);
    for lockfile in std::iter::once(&lock).chain(previous.as_ref()) {
        lockfile
            .check_package(&package)
            .map_err(|err| ContractError::from_error(&err))?;
    }
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
    let paths = resolve_paths(
        Emitter::Rust,
        &options.config,
        options.root.clone(),
        options.out.clone(),
        options.released.clone(),
        None,
        None,
    )?;
    // Generation reproduces released versions without modifying them, so the manifest only has
    // to be valid. The emitter validates every version and lockfile before writing.
    load_root_released_manifest(&paths.root, paths.released.as_deref())?;
    if options.check {
        return Ok(check_messages(&emit::rust::check(&paths.root, &paths.out)?));
    }
    emit::rust::generate(&paths.root, &paths.out)?;
    Ok(ExitCode::SUCCESS)
}

fn proto(options: &ProtoOptions) -> Result<ExitCode> {
    let paths = resolve_paths(
        Emitter::Proto,
        &options.config,
        options.root.clone(),
        options.out.clone(),
        options.released.clone(),
        None,
        None,
    )?;
    load_root_released_manifest(&paths.root, paths.released.as_deref())?;
    if options.check {
        return Ok(check_messages(&emit::proto::check(
            &paths.root,
            &paths.out,
        )?));
    }
    emit::proto::generate(&paths.root, &paths.out)?;
    Ok(ExitCode::SUCCESS)
}

fn schema(options: &SchemaOptions) -> Result<ExitCode> {
    let paths = resolve_paths(
        Emitter::Schema,
        &options.config,
        options.root.clone(),
        options.out.clone(),
        options.released.clone(),
        None,
        None,
    )?;
    load_root_released_manifest(&paths.root, paths.released.as_deref())?;
    if options.check {
        return Ok(check_messages(&emit::json_schema::check(
            &paths.root,
            &paths.out,
        )?));
    }
    emit::json_schema::generate(&paths.root, &paths.out)?;
    Ok(ExitCode::SUCCESS)
}

fn glue(options: &GlueOptions) -> Result<ExitCode> {
    let paths = resolve_paths(
        Emitter::Glue,
        &options.config,
        options.root.clone(),
        options.out.clone(),
        options.released.clone(),
        options.plain_root.clone(),
        options.proto_root.clone(),
    )?;
    let plain_root = paths
        .plain_root
        .as_deref()
        .context("glue requires --plain-root or a config glue section")?;
    let proto_root = paths
        .proto_root
        .as_deref()
        .context("glue requires --proto-root or a config glue section")?;
    // Glue is current-schema output for the highest minor of each major; released plain modules
    // stay frozen. As for the other generators, the manifest only has to be valid.
    load_root_released_manifest(&paths.root, paths.released.as_deref())?;
    if options.check {
        return Ok(check_messages(&emit::glue::check(
            &paths.root,
            &paths.out,
            plain_root,
            proto_root,
        )?));
    }
    emit::glue::generate(&paths.root, &paths.out, plain_root, proto_root)?;
    Ok(ExitCode::SUCCESS)
}

fn check_messages(messages: &[String]) -> ExitCode {
    for message in messages {
        eprintln!("error: {message}");
    }
    if messages.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
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

fn package_name(model: &model::Model) -> PackageName {
    PackageName {
        namespace: model.package.namespace.clone(),
        name: model.package.name.clone(),
    }
}

/// Read the current lockfile and its text. Only `--first` treats a missing one as empty: in
/// default mode a fresh lockfile would renumber a new minor that should continue the previous
/// one, and `--against` develops a copy of the previous lockfile.
fn read_lock(path: &Path, options: &Options) -> Result<(WireLock, Option<String>)> {
    match fs::read_to_string(path) {
        Ok(text) => {
            let lock = WireLock::parse(&text).with_context(|| format!("in {}", path.display()))?;
            Ok((lock, Some(text)))
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            if options.first {
                return Ok((WireLock::default(), None));
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
                 major, create it with `{first}`; for a new minor, copy the previous minor's \
                 wire.lock to {path} and run `{against}`",
                path = path.display(),
                first = lock_command_with(options, Some("--first")),
                against =
                    lock_command_with(options, Some("--against <previous minor's wire.lock>")),
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

/// How to fix what `check` reports: run `lock`, or, for a released version, which `lock`
/// refuses, restore what was released.
enum Remedy {
    /// The `witforge lock` command, with the same mode and paths.
    Lock(String),
    /// The released version directory.
    Released(String),
}

impl Remedy {
    fn advice(&self, purpose: &str) -> String {
        match self {
            Self::Lock(command) => format!("run `{command}`{purpose}"),
            Self::Released(dir) => format!(
                "`{dir}` is released, so `witforge lock` refuses it; restore its released WIT \
                 and wire.lock"
            ),
        }
    }
}

fn report_check_changes(changes: &witforge::lock::Changes, remedy: &Remedy) {
    for (key, _) in &changes.added {
        eprintln!("error: {key} has no wire number; {}", remedy.advice(""));
    }
    for (key, number) in &changes.reserved {
        eprintln!(
            "error: {key} = {number} is no longer in the contract; {}",
            remedy.advice(" to reserve it")
        );
    }
    for (key, number) in &changes.reintroduced {
        eprintln!("error: {}", reintroduced(key, *number));
    }
    for (key, number) in &changes.dropped {
        eprintln!(
            "error: stale draft {key} = {number}; {}",
            remedy.advice(" to drop it")
        );
    }
    for (key, _kind, _ty) in &changes.updated {
        eprintln!(
            "error: draft {key} changed kind or type; {}",
            remedy.advice(" to update it")
        );
    }
}

/// The `witforge lock` command that fixes what `check` reports, with the same mode and paths,
/// each quoted for a POSIX shell.
fn lock_command(options: &Options) -> String {
    let mode = if options.first {
        Some("--first".to_string())
    } else {
        options
            .against
            .as_ref()
            .map(|previous| format!("--against {}", shell_quote(previous)))
    };
    lock_command_with(options, mode.as_deref())
}

/// The `witforge lock` command for `options`' version directory, lockfile, and manifest, with
/// `mode` (such as `--first`) in place of the options' own, each path quoted for a POSIX shell.
fn lock_command_with(options: &Options, mode: Option<&str>) -> String {
    let mut command = format!("witforge lock {}", shell_quote(&options.dir));
    if let Some(lock) = &options.lock {
        let _ = write!(command, " --lock {}", shell_quote(lock));
    }
    if let Some(mode) = mode {
        let _ = write!(command, " {mode}");
    }
    if let Some(released) = &options.released {
        let _ = write!(command, " --released {}", shell_quote(released));
    }
    command
}

/// `path` as one POSIX shell word that witforge reads as a path: unchanged when it holds only
/// characters no shell treats specially, and single-quoted otherwise. A path that starts with
/// `-` is relative, and gets a leading `./`, so the command line does not read it as an option.
fn shell_quote(path: &Path) -> String {
    let mut text = path.to_string_lossy().into_owned();
    if text.starts_with('-') {
        text.insert_str(0, "./");
    }
    let plain = !text.is_empty()
        && text
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"%+,-./:=@_".contains(&byte));
    if plain {
        text
    } else {
        format!("'{}'", text.replace('\'', "'\\''"))
    }
}

fn reintroduced(key: &str, number: u32) -> String {
    format!(
        "{key} was removed and its name and number {number} stay reserved; give the member a new name"
    )
}

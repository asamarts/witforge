//! The `witforge` command-line interface.
//!
//! ```text
//! witforge check <wit-dir> [--lock PATH] [--first | --against PATH] [--released PATH]
//! witforge lock  <wit-dir> [--lock PATH] [--first | --against PATH] [--released PATH]
//! ```

use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand};

use witforge::lock::{LockMode, WireLock};
use witforge::{ensure_not_released, load, model, subset};

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
    /// Released-version manifest; defaults to `<WIT_DIR>/../released`.
    #[arg(long, value_name = "PATH")]
    released: Option<PathBuf>,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(code) => code,
        Err(err) => {
            eprintln!("witforge: {err:#}");
            ExitCode::from(2)
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode> {
    match cli.command {
        Command::Check(options) => check(&options),
        Command::Lock(options) => lock(&options),
    }
}

fn check(options: &Options) -> Result<ExitCode> {
    let contract = load(&options.dir)?;
    let violations = subset::check(&contract);
    for violation in &violations {
        eprintln!("error: {violation}");
    }
    let members = if violations.is_empty() {
        model::members(&contract)?
    } else {
        Vec::new()
    };
    let lock_path = options
        .lock
        .clone()
        .unwrap_or_else(|| options.dir.join("wire.lock"));
    let lock = read_lock(&lock_path, options.against.is_some())?;
    let previous = read_previous(options.against.as_deref())?;
    let mode = lock_mode(options, previous.as_ref());
    let drift = match lock.drift_with(&members, mode) {
        Ok(changes) => changes,
        Err(err) => {
            eprintln!("error: {err:#}");
            return Ok(ExitCode::from(1));
        }
    };
    report_check_changes(&drift);
    if violations.is_empty() && drift.is_empty() {
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
        bail!(
            "{} subset violation(s); the lockfile was not updated",
            violations.len()
        );
    }
    let members = model::members(&contract)?;
    let lock_path = options
        .lock
        .clone()
        .unwrap_or_else(|| options.dir.join("wire.lock"));
    let mut lock = read_lock(&lock_path, options.against.is_some())?;
    let previous = read_previous(options.against.as_deref())?;
    let mode = lock_mode(options, previous.as_ref());
    let changes = lock.reconcile_with(&members, mode)?;
    if !changes.reintroduced.is_empty() {
        for (key, number) in &changes.reintroduced {
            eprintln!("error: {}", reintroduced(key, *number));
        }
        bail!(
            "{} removed member(s) reintroduced; the lockfile was not updated",
            changes.reintroduced.len()
        );
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

fn read_lock(path: &Path, required: bool) -> Result<WireLock> {
    match fs::read_to_string(path) {
        Ok(text) => WireLock::parse(&text).with_context(|| format!("in {}", path.display())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound && !required => {
            Ok(WireLock::default())
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            bail!("lockfile {} does not exist", path.display())
        }
        Err(err) => Err(err).with_context(|| format!("failed to read {}", path.display())),
    }
}

fn read_required_lock(path: &Path) -> Result<WireLock> {
    let text =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    WireLock::parse(&text).with_context(|| format!("in {}", path.display()))
}

fn report_check_changes(changes: &witforge::lock::Changes) {
    for (key, _) in &changes.added {
        eprintln!("error: {key} has no wire number; run `witforge lock`");
    }
    for (key, number) in &changes.reserved {
        eprintln!(
            "error: {key} = {number} is no longer in the contract; run `witforge lock` to reserve it"
        );
    }
    for (key, number) in &changes.reintroduced {
        eprintln!("error: {}", reintroduced(key, *number));
    }
    for (key, number) in &changes.dropped {
        eprintln!("error: stale draft {key} = {number}; run `witforge lock` to drop it");
    }
    for (key, _kind, _ty) in &changes.updated {
        eprintln!("error: draft {key} changed kind or type; run `witforge lock` to update it");
    }
}

fn reintroduced(key: &str, number: u32) -> String {
    format!(
        "{key} was removed and its name and number {number} stay reserved; give the member a new name"
    )
}

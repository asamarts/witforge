//! The `witforge` command-line interface.
//!
//! ```text
//! witforge check <wit-dir> [--lock <path>]   subset rules + lockfile is current
//! witforge lock  <wit-dir> [--lock <path>]   assign numbers to new members, reserve removed ones
//! witforge --help | --version
//! ```
//!
//! The lockfile defaults to `<wit-dir>/wire.lock`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use witforge::lock::WireLock;
use witforge::{load, model, subset};

fn main() -> ExitCode {
    match run(std::env::args().skip(1).collect()) {
        Ok(code) => code,
        Err(err) => {
            eprintln!("witforge: {err:#}");
            ExitCode::from(2)
        }
    }
}

fn run(args: Vec<String>) -> Result<ExitCode> {
    let mut args = args.into_iter();
    let command = args.next().unwrap_or_default();
    if command == "--version" || command == "-V" {
        println!("witforge {}", env!("CARGO_PKG_VERSION"));
        return Ok(ExitCode::SUCCESS);
    }
    let mut dir: Option<PathBuf> = None;
    let mut lock_path: Option<PathBuf> = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                usage();
                return Ok(ExitCode::SUCCESS);
            }
            "--lock" => lock_path = Some(args.next().context("`--lock` needs a path")?.into()),
            _ if dir.is_none() && !arg.starts_with('-') => dir = Some(arg.into()),
            _ => bail!("unexpected argument `{arg}`"),
        }
    }
    match command.as_str() {
        "check" | "lock" => {}
        "" | "-h" | "--help" | "help" => {
            usage();
            return Ok(ExitCode::SUCCESS);
        }
        other => bail!("unknown command `{other}` (expected `check` or `lock`)"),
    }
    let dir = dir.context("missing <wit-dir>")?;
    let lock_path = lock_path.unwrap_or_else(|| dir.join("wire.lock"));
    let contract = load(&dir)?;

    let violations = subset::check(&contract);
    for violation in &violations {
        eprintln!("error: {violation}");
    }
    // A subset violation can make the owned model unbuildable. Report the violations first and
    // leave the member set empty; the command paths below still return the existing exit codes.
    let members = if violations.is_empty() {
        model::members(&contract)?
    } else {
        Vec::new()
    };
    let mut lock = read_lock(&lock_path)?;

    if command == "lock" {
        if !violations.is_empty() {
            bail!(
                "{} subset violation(s); the lockfile was not updated",
                violations.len()
            );
        }
        let changes = lock.reconcile(&members)?;
        if !changes.reintroduced.is_empty() {
            for (key, number) in &changes.reintroduced {
                eprintln!("error: {}", reintroduced(key, *number));
            }
            bail!(
                "{} removed member(s) reintroduced; the lockfile was not updated",
                changes.reintroduced.len()
            );
        }
        for (key, number) in &changes.added {
            println!("added    {key} = {number}");
        }
        for (key, number) in &changes.reserved {
            println!("reserved {key} = {number}");
        }
        lock.write_atomic(&lock_path)?;
        return Ok(ExitCode::SUCCESS);
    }

    let drift = lock.drift(&members)?;
    for (key, _) in &drift.added {
        eprintln!("error: {key} has no wire number; run `witforge lock`");
    }
    for (key, number) in &drift.reserved {
        eprintln!(
            "error: {key} = {number} is no longer in the contract; run `witforge lock` to reserve it"
        );
    }
    for (key, number) in &drift.reintroduced {
        eprintln!("error: {}", reintroduced(key, *number));
    }
    if violations.is_empty() && drift.is_empty() {
        Ok(ExitCode::SUCCESS)
    } else {
        Ok(ExitCode::from(1))
    }
}

fn usage() {
    println!(
        "usage: witforge <check|lock> <wit-dir> [--lock <path>]\n       witforge --help | --version"
    );
}

fn reintroduced(key: &str, number: u32) -> String {
    format!(
        "{key} was removed and its name and number {number} stay reserved; give the member a new name"
    )
}

fn read_lock(path: &Path) -> Result<WireLock> {
    match std::fs::read_to_string(path) {
        Ok(text) => WireLock::parse(&text).with_context(|| format!("in {}", path.display())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(WireLock::default()),
        Err(err) => Err(err).with_context(|| format!("failed to read {}", path.display())),
    }
}

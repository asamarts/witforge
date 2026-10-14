//! The `witforge` command-line interface.
//!
//! ```text
//! witforge check <wit-dir> [--lock <path>]   subset rules + lockfile is current
//! witforge lock  <wit-dir> [--lock <path>]   assign numbers to new members, reserve removed ones
//! ```
//!
//! The lockfile defaults to `<wit-dir>/wire.lock`.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{bail, Context, Result};
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
    let mut dir: Option<PathBuf> = None;
    let mut lock_path: Option<PathBuf> = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--lock" => lock_path = Some(args.next().context("`--lock` needs a path")?.into()),
            _ if dir.is_none() && !arg.starts_with('-') => dir = Some(arg.into()),
            _ => bail!("unexpected argument `{arg}`"),
        }
    }
    match command.as_str() {
        "check" | "lock" => {}
        "" | "-h" | "--help" | "help" => {
            println!("usage: witforge <check|lock> <wit-dir> [--lock <path>]");
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
    let members = model::members(&contract);
    let mut lock = read_lock(&lock_path)?;

    if command == "lock" {
        if !violations.is_empty() {
            bail!(
                "{} subset violation(s); the lockfile was not updated",
                violations.len()
            );
        }
        let changes = lock.reconcile(&members);
        for (key, number) in &changes.added {
            println!("added    {key} = {number}");
        }
        for (key, number) in &changes.reserved {
            println!("reserved {key} = {number}");
        }
        std::fs::write(&lock_path, lock.render())
            .with_context(|| format!("failed to write {}", lock_path.display()))?;
        return Ok(ExitCode::SUCCESS);
    }

    let drift = lock.drift(&members);
    for (key, _) in &drift.added {
        eprintln!("error: {key} has no wire number; run `witforge lock`");
    }
    for (key, number) in &drift.reserved {
        eprintln!("error: {key} = {number} is no longer in the contract; run `witforge lock` to reserve it");
    }
    if violations.is_empty() && drift.is_empty() {
        Ok(ExitCode::SUCCESS)
    } else {
        Ok(ExitCode::from(1))
    }
}

fn read_lock(path: &Path) -> Result<WireLock> {
    match std::fs::read_to_string(path) {
        Ok(text) => WireLock::parse(&text).with_context(|| format!("in {}", path.display())),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(WireLock::default()),
        Err(err) => Err(err).with_context(|| format!("failed to read {}", path.display())),
    }
}

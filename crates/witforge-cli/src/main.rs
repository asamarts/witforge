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
//! witforge diff  <old> <new> [--json] [--check]
//!
//! All commands accept `--format human|json|sarif`; `human` is the default.
//! ```

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{Args, Parser, Subcommand, ValueEnum};
use serde::Deserialize;

use serde_json::{Value, json};
use witforge::diagnostic::{Diagnostic, Severity};
use witforge::diff::{self, ChangeKind, DiffReport};
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
    /// Emit diagnostics as human text, JSON, or SARIF 2.1.0.
    #[arg(long, global = true, value_enum, default_value_t = Format::Human)]
    format: Format,
    /// Command to run.
    #[command(subcommand)]
    command: Command,
}

/// The diagnostic output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Format {
    /// Human-readable diagnostics on stderr; the default.
    Human,
    /// A versioned JSON diagnostic envelope on stdout.
    Json,
    /// SARIF 2.1.0 on stdout.
    Sarif,
}

struct Reporter {
    format: Format,
    diagnostics: Vec<Diagnostic>,
    suppressed: bool,
}

impl Reporter {
    fn new(format: Format) -> Self {
        Self {
            format,
            diagnostics: Vec::new(),
            suppressed: false,
        }
    }

    fn human(&self) -> bool {
        self.format == Format::Human
    }

    fn suppress(&mut self) {
        self.suppressed = true;
    }

    fn push(&mut self, diagnostic: Diagnostic) {
        if self.human() {
            eprintln!("error: {}", diagnostic.message);
        } else {
            self.diagnostics.push(diagnostic);
        }
    }

    fn error(&mut self, code: &'static str, message: impl Into<String>, path: Option<String>) {
        let mut diagnostic = Diagnostic::error(code, message);
        if let Some(path) = path {
            diagnostic = diagnostic.with_path(path);
        }
        self.push(diagnostic);
    }

    fn error_with_location(
        &mut self,
        code: &'static str,
        location: impl Into<String>,
        message: impl Into<String>,
    ) {
        let location = location.into();
        if self.human() {
            eprintln!("error: {location}: {}", message.into());
        } else {
            self.diagnostics
                .push(Diagnostic::error(code, message).with_path(location));
        }
    }

    fn note(&mut self, code: &'static str, human: &str, message: impl Into<String>, path: String) {
        if self.human() {
            println!("{human}");
        } else {
            self.diagnostics
                .push(Diagnostic::new(code, Severity::Note, message).with_path(path));
        }
    }

    fn finish(&self) -> Result<()> {
        if self.suppressed || self.human() {
            return Ok(());
        }
        match self.format {
            Format::Human => Ok(()),
            Format::Json => {
                println!("{}", diagnostic_json(&self.diagnostics)?);
                Ok(())
            }
            Format::Sarif => {
                println!("{}", sarif_json(&self.diagnostics)?);
                Ok(())
            }
        }
    }
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
    /// Compare two contract versions and classify breaking changes.
    Diff(DiffOptions),
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
struct DiffOptions {
    /// Old version directory or WIT root.
    #[arg(value_name = "OLD")]
    old: PathBuf,
    /// New version directory or WIT root.
    #[arg(value_name = "NEW")]
    new: PathBuf,
    /// Emit machine-readable JSON instead of the human report.
    #[arg(long)]
    json: bool,
    /// Exit non-zero when any wire, source, or generated-API breaking change is found.
    #[arg(long)]
    check: bool,
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
    let mut reporter = Reporter::new(cli.format);
    let result = match cli.command {
        Command::Check(options) => check(&options, &mut reporter),
        Command::Lock(options) => lock(&options, &mut reporter),
        Command::Rust(options) => rust(&options, &mut reporter),
        Command::Proto(options) => proto(&options, &mut reporter),
        Command::Schema(options) => schema(&options, &mut reporter),
        Command::Glue(options) => glue(&options, &mut reporter),
        Command::Diff(options) => diff_command(&options, &mut reporter),
    };
    match result {
        Ok(code) => {
            reporter.finish()?;
            Ok(code)
        }
        Err(error) => {
            if reporter.human() {
                return Err(error);
            }
            reporter.push(Diagnostic::error("WF9000", format!("{error:#}")));
            reporter.finish()?;
            Ok(ExitCode::from(if is_contract_error(&error) {
                1
            } else {
                2
            }))
        }
    }
}

fn check(options: &Options, reporter: &mut Reporter) -> Result<ExitCode> {
    ensure_version_directory(&options.dir)?;
    // `check` may verify a released directory; the manifest only has to be valid.
    load_released_manifest(&options.dir, options.released.as_deref())?;
    let contract = load(&options.dir)?;
    let violations = subset::check(&contract);
    if !violations.is_empty() {
        // The model of a contract outside the subset is not built, so every locked member
        // would look removed; report only the violations, as `lock` does.
        for violation in &violations {
            reporter.error_with_location("WF1001", &violation.location, &violation.message);
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
            reporter.error(
                "WF1002",
                format!("{err:#}"),
                Some(lock_path.display().to_string()),
            );
            return Ok(ExitCode::from(1));
        }
    }
    let mode = lock_mode(options, previous.as_ref());
    let drift = match lock.drift_with(&members, mode) {
        Ok(changes) => changes,
        Err(err) => {
            reporter.error(
                "WF1100",
                format!("{err:#}"),
                Some(lock_path.display().to_string()),
            );
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
        reporter.error(
            "WF1106",
            format!(
                "lockfile {} is not in the form `witforge lock` writes; {}",
                lock_path.display(),
                remedy.advice(" to rewrite it")
            ),
            Some(lock_path.display().to_string()),
        );
    }
    report_check_changes(&drift, &remedy, reporter);
    if drift.is_empty() && canonical {
        Ok(ExitCode::SUCCESS)
    } else {
        Ok(ExitCode::from(1))
    }
}

fn lock(options: &Options, reporter: &mut Reporter) -> Result<ExitCode> {
    ensure_not_released(&options.dir, options.released.as_deref())?;
    let contract = load(&options.dir)?;
    let violations = subset::check(&contract);
    for violation in &violations {
        reporter.error_with_location("WF1001", &violation.location, &violation.message);
    }
    if !violations.is_empty() {
        if reporter.human() {
            bail!(ContractError::new(format!(
                "{} subset violation(s); the lockfile was not updated",
                violations.len()
            )));
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
            reporter.error("WF1103", reintroduced(key, *number), Some(key.clone()));
        }
        if reporter.human() {
            bail!(ContractError::new(format!(
                "{} removed member(s) reintroduced; the lockfile was not updated",
                changes.reintroduced.len()
            )));
        }
        return Ok(ExitCode::from(1));
    }
    for (key, number) in &changes.dropped {
        reporter.note(
            "WF1203",
            &format!("dropped  {key} = {number}"),
            format!("dropped {key} = {number}"),
            key.clone(),
        );
    }
    for (key, _kind, _ty) in &changes.updated {
        reporter.note(
            "WF1204",
            &format!("updated  {key}"),
            format!("updated {key}"),
            key.clone(),
        );
    }
    for (key, number) in &changes.reserved {
        reporter.note(
            "WF1202",
            &format!("reserved {key} = {number}"),
            format!("reserved {key} = {number}"),
            key.clone(),
        );
    }
    for (key, number) in &changes.added {
        reporter.note(
            "WF1201",
            &format!("added    {key} = {number}"),
            format!("added {key} = {number}"),
            key.clone(),
        );
    }
    lock.write_atomic(&lock_path)?;
    Ok(ExitCode::SUCCESS)
}

fn rust(options: &RustOptions, reporter: &mut Reporter) -> Result<ExitCode> {
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
        return Ok(check_messages(
            &emit::rust::check(&paths.root, &paths.out)?,
            reporter,
        ));
    }
    emit::rust::generate(&paths.root, &paths.out)?;
    Ok(ExitCode::SUCCESS)
}

fn proto(options: &ProtoOptions, reporter: &mut Reporter) -> Result<ExitCode> {
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
        return Ok(check_messages(
            &emit::proto::check(&paths.root, &paths.out)?,
            reporter,
        ));
    }
    emit::proto::generate(&paths.root, &paths.out)?;
    Ok(ExitCode::SUCCESS)
}

fn schema(options: &SchemaOptions, reporter: &mut Reporter) -> Result<ExitCode> {
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
        return Ok(check_messages(
            &emit::json_schema::check(&paths.root, &paths.out)?,
            reporter,
        ));
    }
    emit::json_schema::generate(&paths.root, &paths.out)?;
    Ok(ExitCode::SUCCESS)
}

fn glue(options: &GlueOptions, reporter: &mut Reporter) -> Result<ExitCode> {
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
        return Ok(check_messages(
            &emit::glue::check(&paths.root, &paths.out, plain_root, proto_root)?,
            reporter,
        ));
    }
    emit::glue::generate(&paths.root, &paths.out, plain_root, proto_root)?;
    Ok(ExitCode::SUCCESS)
}

fn diff_command(options: &DiffOptions, reporter: &mut Reporter) -> Result<ExitCode> {
    let report = diff::compare(&options.old, &options.new)?;
    if options.json {
        // `--json` is the diff report; it takes precedence over the diagnostic envelope.
        reporter.suppress();
        println!(
            "{}",
            serde_json::to_string_pretty(&diff_json(&report))
                .context("failed to render the diff as JSON")?
        );
    } else if reporter.human() {
        println!("{}", render_diff(&report));
    } else {
        for change in &report.changes {
            let (code, severity) = if change.is_breaking() {
                ("WF3001", Severity::Error)
            } else {
                ("WF3002", Severity::Note)
            };
            let (kind, detail) = change_label(change);
            let mut message = format!("{} {} {kind}", change.element, change.path);
            if let Some(number) = change.number {
                let _ = write!(message, " number {number}");
            }
            if !detail.is_empty() {
                let _ = write!(message, ": {detail}");
            }
            reporter.push(Diagnostic::new(code, severity, message).with_path(change.path.clone()));
        }
    }
    if options.check && report.summary.breaking > 0 {
        if reporter.human() {
            eprintln!(
                "diff: {} breaking change(s); --check failed",
                report.summary.breaking
            );
        }
        return Ok(ExitCode::from(1));
    }
    Ok(ExitCode::SUCCESS)
}

fn diff_json(report: &DiffReport) -> Value {
    let endpoint = |endpoint: &diff::Endpoint| {
        json!({
            "path": endpoint.path.display().to_string(),
            "package": endpoint.package,
            "version": endpoint.version.to_string(),
        })
    };
    let changes: Vec<Value> = report.changes.iter().map(change_json).collect();
    json!({
        "old": endpoint(&report.old),
        "new": endpoint(&report.new),
        "changes": changes,
        "summary": {
            "changes": report.summary.changes,
            "breaking": report.summary.breaking,
            "wire_breaking": report.summary.wire_breaking,
            "source_breaking": report.summary.source_breaking,
            "api_breaking": report.summary.api_breaking,
        },
    })
}

fn change_json(change: &diff::Change) -> Value {
    let mut value = json!({
        "element": &change.element,
        "path": &change.path,
        "number": change.number,
        "wire_breaking": change.wire_breaking,
        "source_breaking": change.source_breaking,
        "api_breaking": change.api_breaking,
    });
    let object = value.as_object_mut().expect("JSON object");
    let (kind, extra) = match &change.kind {
        ChangeKind::Added { defaulted } => ("added", json!({ "defaulted": defaulted })),
        ChangeKind::Removed => ("removed", json!({})),
        ChangeKind::Reordered { before, after } => {
            ("reordered", json!({ "before": before, "after": after }))
        }
        ChangeKind::Retyped { from, to } | ChangeKind::KindChanged { from, to } => {
            let kind = if matches!(change.kind, ChangeKind::Retyped { .. }) {
                "retyped"
            } else {
                "kind-changed"
            };
            (kind, json!({ "from": from, "to": to }))
        }
        ChangeKind::Renumbered { from, to } => ("renumbered", json!({ "from": from, "to": to })),
    };
    object.insert("kind".to_string(), Value::String(kind.to_string()));
    if let Some(extra) = extra.as_object() {
        for (key, value) in extra {
            object.insert(key.clone(), value.clone());
        }
    }
    value
}

fn change_label(change: &diff::Change) -> (&'static str, String) {
    match &change.kind {
        ChangeKind::Added { defaulted: true } => ("added-defaulted", String::new()),
        ChangeKind::Added { defaulted: false } => ("added", String::new()),
        ChangeKind::Removed => ("removed", String::new()),
        ChangeKind::Reordered { before, after } => (
            "reordered",
            format!("{} -> {}", before.join(", "), after.join(", ")),
        ),
        ChangeKind::Retyped { from, to } => ("retyped", format!("{from} -> {to}")),
        ChangeKind::KindChanged { from, to } => ("kind-changed", format!("{from} -> {to}")),
        ChangeKind::Renumbered { from, to } => ("renumbered", format!("{from} -> {to}")),
    }
}

fn render_diff(report: &DiffReport) -> String {
    use std::fmt::Write as _;

    let mut out = String::new();
    let _ = writeln!(
        out,
        "old: {} ({} {})",
        report.old.path.display(),
        report.old.package,
        report.old.version
    );
    let _ = writeln!(
        out,
        "new: {} ({} {})",
        report.new.path.display(),
        report.new.package,
        report.new.version
    );
    if report.changes.is_empty() {
        let _ = writeln!(out, "no contract changes");
    }
    for change in &report.changes {
        let (kind, detail) = change_label(change);
        let number = change
            .number
            .map_or_else(String::new, |number| format!(" number={number}"));
        let _ = writeln!(
            out,
            "{kind:<17} {:<18} {:<32}{number:<10}  wire:{} source:{} api:{}  {detail}",
            change.element,
            change.path,
            status(change.wire_breaking),
            status(change.source_breaking),
            status(change.api_breaking),
        );
    }
    let _ = writeln!(
        out,
        "summary: {} change(s), {} breaking; wire identity: {}, source: {}, generated API: {}",
        report.summary.changes,
        report.summary.breaking,
        report.summary.wire_breaking,
        report.summary.source_breaking,
        report.summary.api_breaking,
    );
    if report.summary.breaking > 0 {
        let _ = writeln!(out, "result: BREAKING");
    } else {
        let _ = writeln!(out, "result: COMPATIBLE");
    }
    out
}

fn status(breaking: bool) -> &'static str {
    if breaking { "BREAKING" } else { "compatible" }
}

fn diagnostic_json(diagnostics: &[Diagnostic]) -> Result<String> {
    let values: Vec<Value> = diagnostics.iter().map(diagnostic_value).collect();
    serde_json::to_string_pretty(&json!({
        "version": 1,
        "tool": {
            "name": "witforge",
            "version": env!("CARGO_PKG_VERSION"),
        },
        "diagnostics": values,
    }))
    .context("failed to render diagnostics as JSON")
}

fn diagnostic_value(diagnostic: &Diagnostic) -> Value {
    let mut value = json!({
        "code": diagnostic.code,
        "severity": diagnostic.severity.as_str(),
        "message": &diagnostic.message,
    });
    let object = value.as_object_mut().expect("JSON object");
    if let Some(path) = &diagnostic.path {
        object.insert("path".to_string(), Value::String(path.clone()));
    }
    if let Some(position) = diagnostic.position {
        object.insert("line".to_string(), json!(position.line));
        object.insert("column".to_string(), json!(position.column));
        object.insert("end_line".to_string(), json!(position.end_line));
        object.insert("end_column".to_string(), json!(position.end_column));
    }
    value
}

fn sarif_json(diagnostics: &[Diagnostic]) -> Result<String> {
    let mut rules = BTreeSet::new();
    for diagnostic in diagnostics {
        rules.insert(diagnostic.code);
    }
    let rules: Vec<Value> = rules
        .into_iter()
        .map(|code| {
            json!({
                "id": code,
                "shortDescription": { "text": code },
            })
        })
        .collect();
    let results: Vec<Value> = diagnostics.iter().map(sarif_result).collect();
    serde_json::to_string_pretty(&json!({
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "version": "2.1.0",
        "runs": [{
            "tool": {
                "driver": {
                    "name": "witforge",
                    "version": env!("CARGO_PKG_VERSION"),
                    "informationUri": "https://github.com/asamarts/witforge",
                    "rules": rules,
                }
            },
            "results": results,
        }],
    }))
    .context("failed to render diagnostics as SARIF")
}

fn sarif_result(diagnostic: &Diagnostic) -> Value {
    let mut result = json!({
        "ruleId": diagnostic.code,
        "level": diagnostic.severity.as_str(),
        "message": { "text": &diagnostic.message },
    });
    if let Some(path) = &diagnostic.path {
        let mut physical = json!({
            "artifactLocation": { "uri": path.replace('\\', "/") },
        });
        if let (Some(position), Some(object)) = (diagnostic.position, physical.as_object_mut()) {
            object.insert(
                "region".to_string(),
                json!({
                    "startLine": position.line,
                    "startColumn": position.column,
                    "endLine": position.end_line,
                    "endColumn": position.end_column,
                }),
            );
        }
        result["locations"] = json!([{ "physicalLocation": physical }]);
    }
    result
}

fn diagnostic_path(message: &str) -> Option<String> {
    let start = message.find('`')?;
    let rest = &message[start + 1..];
    let end = rest.find('`')?;
    Some(rest[..end].to_string())
}

fn check_messages(messages: &[String], reporter: &mut Reporter) -> ExitCode {
    for message in messages {
        reporter.error("WF2001", message, diagnostic_path(message));
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

fn report_check_changes(
    changes: &witforge::lock::Changes,
    remedy: &Remedy,
    reporter: &mut Reporter,
) {
    for (key, _) in &changes.added {
        reporter.error(
            "WF1101",
            format!("{key} has no wire number; {}", remedy.advice("")),
            Some(key.clone()),
        );
    }
    for (key, number) in &changes.reserved {
        reporter.error(
            "WF1102",
            format!(
                "{key} = {number} is no longer in the contract; {}",
                remedy.advice(" to reserve it")
            ),
            Some(key.clone()),
        );
    }
    for (key, number) in &changes.reintroduced {
        reporter.error("WF1103", reintroduced(key, *number), Some(key.clone()));
    }
    for (key, number) in &changes.dropped {
        reporter.error(
            "WF1104",
            format!(
                "stale draft {key} = {number}; {}",
                remedy.advice(" to drop it")
            ),
            Some(key.clone()),
        );
    }
    for (key, _kind, _ty) in &changes.updated {
        reporter.error(
            "WF1105",
            format!(
                "draft {key} changed kind or type; {}",
                remedy.advice(" to update it")
            ),
            Some(key.clone()),
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

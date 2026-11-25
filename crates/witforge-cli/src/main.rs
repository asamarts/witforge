//! The `witforge` command-line interface.
//!
//! ```text
//! witforge check [<path>] [--lock PATH] [--first | --against PATH] [--released PATH]
//!                [--config PATH | --no-config] [--project NAME]
//! witforge lock  <wit-dir> [--lock PATH] [--first | --against PATH] [--released PATH]
//! witforge rust  [<wit-root>] [--out DIR] [--config PATH | --no-config] [--project NAME]
//!                [--serde | --no-serde] [--no-std | --std] [--check] [--diff] [--released PATH]
//! witforge proto [<wit-root>] [--out DIR] [--config PATH | --no-config] [--project NAME]
//!                [--check] [--diff] [--released PATH]
//! witforge schema [<wit-root>] [--out DIR] [--draft 7|2020-12] [--config PATH | --no-config]
//!                [--project NAME] [--check] [--diff] [--released PATH]
//! witforge glue  [<wit-root>] [--out DIR] [--plain-root PATH] [--proto-root PATH]
//!                [--no-std | --std] [--config PATH | --no-config] [--project NAME] [--check]
//!                [--diff] [--released PATH]
//! witforge generate [--config PATH | --no-config] [--project NAME] [--check] [--diff]
//!                   [--serde | --no-serde] [--no-std | --std] [--draft 7|2020-12] [--released PATH]
//! witforge init  [DIR] [--package NS:NAME] [--serde]
//! witforge new-version [<wit-root>] [--major] [--from VERSION] [--released PATH]
//!                      [--config PATH | --no-config] [--project NAME]
//! witforge release <wit-dir> [--released PATH] [--config PATH | --no-config] [--project NAME]
//! witforge retire  <wit-dir> [--released PATH] [--config PATH | --no-config] [--project NAME]
//! witforge diff  <old> <new> [--json] [--check[=VIEWS]]
//! witforge inspect [<path>] [--json] [--released PATH] [--config PATH | --no-config] [--project NAME]
//! witforge freeze [<wit-root>] [--released PATH] [--config PATH | --no-config] [--project NAME] [--base REV]
//! witforge completions <bash|zsh|fish>
//! witforge explain <code-id>
//!
//! All commands accept `--format human|json|sarif` (`human` default) and `-q`/`--quiet`.
//! ```

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{Args, CommandFactory, Parser, Subcommand};
use serde::Deserialize;

use serde_json::{Value, json};
use witforge::diagnostic::{Diagnostic, LocatedDiagnostic, Severity, codes};
use witforge::diff::{self, ChangeElement, ChangeKind, DiffReport};
use witforge::emit;
use witforge::workflow as contract_workflow;
mod completions;
mod diagnostics;
mod workflow;

use diagnostics::{Format, Reporter, located_note};

use witforge::{
    ContractError, DiagnosticError, ReleasedManifest, default_released_manifest,
    discover_version_directories, ensure_no_symlinks_in_dir, ensure_version_directory,
    error_diagnostics, error_located_diagnostics, is_contract_error, load_root_released_manifest,
};

fn op_error(code: &'static str, message: impl Into<String>, path: Option<String>) -> anyhow::Error {
    let mut diagnostic = Diagnostic::error(code, message);
    let file = path.clone();
    if let Some(path) = path {
        diagnostic = diagnostic.with_path(path);
    }
    let mut located = LocatedDiagnostic::from_diagnostic(diagnostic);
    if let Some(file) = file {
        located = located.with_file(file);
    }
    anyhow::Error::new(DiagnosticError::single_located(located))
}

/// The witforge contract compiler.
#[derive(Debug, Parser)]
#[command(name = "witforge", version = env!("WITFORGE_BUILD_VERSION"), about)]
struct Cli {
    /// Emit diagnostics as human text, JSON, or SARIF 2.1.0.
    #[arg(long, global = true, value_enum, default_value_t = Format::Human)]
    format: Format,
    /// Suppress human notes and summaries (never hides errors or JSON/SARIF output).
    #[arg(short = 'q', long, global = true)]
    quiet: bool,
    /// Command to run.
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Check a version directory or an entire WIT root without writing.
    Check(CheckOptions),
    /// Assign wire numbers to new members and reserve removed ones.
    Lock(Options),
    /// Generate plain Rust types and interface traits.
    Rust(RustOptions),
    /// Generate protobuf schemas.
    Proto(ProtoOptions),
    /// Generate JSON Schema definitions (Draft 7 default, opt-in Draft 2020-12).
    Schema(SchemaOptions),
    /// Generate protobuf glue conversions and function helpers.
    Glue(GlueOptions),
    /// Run every generator configured in `witforge.toml`.
    Generate(GenerateOptions),
    /// Scaffold a starter `wit/v0.1/contract.wit`, initial `wire.lock`, and `witforge.toml`.
    Init(InitOptions),
    /// Create the next minor or major version directory from the latest (or `--from`) version.
    #[command(name = "new-version")]
    NewVersion(NewVersionOptions),
    /// Validate a version directory and append it to the `released` manifest.
    Release(ReleaseOptions),
    /// Mark a released version directory as `retired` in the `released` manifest.
    Retire(RetireOptions),
    /// Compare two contract versions and classify breaking changes.
    Diff(DiffOptions),
    /// Inspect a contract version directory or WIT root (human summary or versioned JSON IR).
    Inspect(InspectOptions),
    /// Verify that released version directories have not changed in Git history.
    Freeze(FreezeOptions),
    /// Generate shell completion scripts for bash, zsh, or fish.
    Completions(CompletionsOptions),
    /// Print the documentation entry for a diagnostic code (for example `WF1015`).
    Explain(ExplainOptions),
}

#[allow(clippy::struct_excessive_bools)]
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
    /// Print a unified diff of generated output changes without changing the exit status.
    #[arg(long)]
    diff: bool,
    /// Add serde Serialize/Deserialize derives to generated types.
    #[arg(long, conflicts_with = "no_serde")]
    serde: bool,
    /// Do not add serde derives, overriding a configuration-file setting.
    #[arg(long = "no-serde", conflicts_with = "serde")]
    no_serde: bool,
    /// Emit `#![no_std]` + `alloc` compatible Rust paths (`::alloc::*`, `::core::*`).
    #[arg(long = "no-std", conflicts_with = "std_mode")]
    no_std: bool,
    /// Emit default `std` Rust paths, overriding a configuration-file `no-std = true` setting.
    #[arg(long = "std", conflicts_with = "no_std")]
    std_mode: bool,
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
    /// Print a unified diff of generated output changes without changing the exit status.
    #[arg(long)]
    diff: bool,
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
    /// JSON Schema dialect (`7` or `2020-12`; defaults to `7` or `schema.draft` in config).
    #[arg(long, value_name = "DRAFT")]
    draft: Option<String>,
    /// Check generated output without writing.
    #[arg(long)]
    check: bool,
    /// Print a unified diff of generated output changes without changing the exit status.
    #[arg(long)]
    diff: bool,
    /// Released-version manifest to validate; defaults to `<WIT_ROOT>/released`.
    #[arg(long, value_name = "PATH")]
    released: Option<PathBuf>,
    /// Configuration file with a project root and emitter outputs.
    #[command(flatten)]
    config: ConfigArgs,
}

#[allow(clippy::struct_excessive_bools)]
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
    /// Emit `#![no_std]` + `alloc` compatible Rust paths (`::alloc::*`, `::core::*`).
    #[arg(long = "no-std", conflicts_with = "std_mode")]
    no_std: bool,
    /// Emit default `std` Rust paths, overriding a configuration-file `no-std = true` setting.
    #[arg(long = "std", conflicts_with = "no_std")]
    std_mode: bool,
    /// Check generated output without writing.
    #[arg(long)]
    check: bool,
    /// Print a unified diff of generated output changes without changing the exit status.
    #[arg(long)]
    diff: bool,
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
    /// Exit non-zero when a breaking change hits any gated view (comma-separated: wire, source,
    /// api, decode; defaults to wire,source,api without decode when given without `=...`).
    #[arg(
        long,
        value_name = "VIEWS",
        num_args = 0..=1,
        require_equals = true,
        default_missing_value = "wire,source,api"
    )]
    check: Option<String>,
}

#[derive(Debug, Args)]
struct CompletionsOptions {
    /// Target shell (`bash`, `zsh`, or `fish`).
    #[arg(value_enum)]
    shell: completions::Shell,
}

#[derive(Debug, Args)]
struct ExplainOptions {
    /// Diagnostic code to explain (for example `WF1015`).
    #[arg(value_name = "CODE")]
    code: String,
}

#[derive(Debug, Args)]
struct InspectOptions {
    /// WIT version directory or WIT root; omit to use the configured project root.
    #[arg(value_name = "PATH")]
    path: Option<PathBuf>,
    /// Emit the versioned JSON contract IR (`version: 1`) on stdout.
    #[arg(long)]
    json: bool,
    /// Released-version manifest path (WIT root only; defaults to `<WIT_ROOT>/released`).
    #[arg(long, value_name = "PATH")]
    released: Option<PathBuf>,
    /// Configuration file; used when PATH is omitted or `--config`/`--project` is passed.
    #[command(flatten)]
    config: ConfigArgs,
}

#[derive(Debug, Args)]
struct FreezeOptions {
    /// WIT root containing version directories and `released` manifest.
    #[arg(value_name = "WIT_ROOT")]
    root: Option<PathBuf>,
    /// Released-version manifest to verify; defaults to `<WIT_ROOT>/released` or project `released`.
    #[arg(long, value_name = "PATH")]
    released: Option<PathBuf>,
    /// Lower-bound Git revision to bound the manifest history scan (default: full history).
    #[arg(long, value_name = "REV")]
    base: Option<String>,
    /// Configuration file with one or more projects whose WIT roots to verify.
    #[command(flatten)]
    config: ConfigArgs,
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Args)]
struct GenerateOptions {
    /// Check all configured generated outputs without writing.
    #[arg(long)]
    check: bool,
    /// Print a unified diff of generated output changes without changing the exit status.
    #[arg(long)]
    diff: bool,
    /// Add serde Serialize/Deserialize derives to generated Rust types.
    #[arg(long, conflicts_with = "no_serde")]
    serde: bool,
    /// Do not add serde derives, overriding a configuration-file setting.
    #[arg(long = "no-serde", conflicts_with = "serde")]
    no_serde: bool,
    /// Emit `#![no_std]` + `alloc` compatible paths across configured Rust and glue outputs.
    #[arg(long = "no-std", conflicts_with = "std_mode")]
    no_std: bool,
    /// Emit default `std` paths across configured Rust and glue outputs, overriding config.
    #[arg(long = "std", conflicts_with = "no_std")]
    std_mode: bool,
    /// JSON Schema dialect for configured schema outputs (`7` or `2020-12`), overriding config.
    #[arg(long, value_name = "DRAFT")]
    draft: Option<String>,
    /// Released-version manifest to validate; overrides a project's `released` setting.
    #[arg(long, value_name = "PATH")]
    released: Option<PathBuf>,
    /// Configuration file; discovered upwards from the working directory when omitted.
    #[command(flatten)]
    config: ConfigArgs,
}

#[derive(Debug, Args)]
struct InitOptions {
    /// Target directory to initialize (defaults to the current directory).
    #[arg(value_name = "DIR")]
    dir: Option<PathBuf>,
    /// WIT package identity (`<namespace>:<name>`, for example `example:hello`).
    #[arg(long, value_name = "NS:NAME")]
    package: Option<String>,
    /// Enable `serde = true` in the generated `witforge.toml` Rust section.
    #[arg(long)]
    serde: bool,
}

#[derive(Debug, Args)]
struct NewVersionOptions {
    /// WIT root containing version directories; omit to use the configured project root.
    #[arg(value_name = "WIT_ROOT")]
    root: Option<PathBuf>,
    /// Start the next major version (`v<major+1>.0`) with no copied wire.lock instead of the next minor.
    #[arg(long)]
    major: bool,
    /// Source version directory to clone (for example `v0.2`), instead of the globally latest version.
    #[arg(long, value_name = "VERSION")]
    from: Option<String>,
    /// Path to the released-version manifest (defaults to `<WIT_ROOT>/released`).
    #[arg(long, value_name = "PATH")]
    released: Option<PathBuf>,
    /// Configuration file; discovered upwards from the working directory when omitted.
    #[command(flatten)]
    config: ConfigArgs,
}

#[derive(Debug, Args)]
struct ReleaseOptions {
    /// WIT version directory to validate and record in `released`.
    #[arg(value_name = "WIT_DIR")]
    dir: PathBuf,
    /// Released-version manifest path; defaults to `<WIT_DIR>/../released`.
    #[arg(long, value_name = "PATH")]
    released: Option<PathBuf>,
    /// Configuration file; read only when `--config` or `--project` is passed.
    #[command(flatten)]
    config: ConfigArgs,
}

#[derive(Debug, Args)]
struct RetireOptions {
    /// Released WIT version directory to mark as `retired` in `released`.
    #[arg(value_name = "WIT_DIR")]
    dir: PathBuf,
    /// Released-version manifest path; defaults to `<WIT_DIR>/../released`.
    #[arg(long, value_name = "PATH")]
    released: Option<PathBuf>,
    /// Configuration file; read only when `--config` or `--project` is passed.
    #[command(flatten)]
    config: ConfigArgs,
}

#[derive(Debug, Args)]
struct CheckOptions {
    /// WIT version directory or WIT root; omit to check the configured project root(s).
    #[arg(value_name = "PATH")]
    dir: Option<PathBuf>,
    /// Lockfile path (single version directory only); defaults to `<WIT_DIR>/wire.lock`.
    #[arg(long, value_name = "PATH")]
    lock: Option<PathBuf>,
    /// Treat entries from this previous lockfile as released (single version directory only).
    #[arg(long, value_name = "PATH", conflicts_with = "first")]
    against: Option<PathBuf>,
    /// Treat every existing entry as a draft (single version directory only).
    #[arg(long, conflicts_with = "against")]
    first: bool,
    /// Released-version manifest to validate; defaults to `<WIT_DIR>/../released` or
    /// `<WIT_ROOT>/released`.
    #[arg(long, value_name = "PATH")]
    released: Option<PathBuf>,
    /// Configuration file; used when PATH is omitted or `--config`/`--project` is passed.
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

/// Configuration-file selection shared by config-aware commands.
#[derive(Debug, Args)]
struct ConfigArgs {
    /// TOML configuration file; paths inside it are relative to the file.
    #[arg(long, value_name = "PATH", conflicts_with = "no_config")]
    config: Option<PathBuf>,
    /// Disable upward discovery of `witforge.toml`.
    #[arg(long = "no-config", conflicts_with = "config")]
    no_config: bool,
    /// Project name in the configuration file; required when a single project must be selected.
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
    rust: Option<RustConfig>,
    proto: Option<OutConfig>,
    schema: Option<SchemaConfig>,
    glue: Option<GlueConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct OutConfig {
    out: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct RustConfig {
    out: PathBuf,
    /// Add serde derives to generated records, enums, variants, and flags.
    serde: Option<bool>,
    /// Emit `#![no_std]` + `alloc` compatible paths instead of `::std::*`.
    #[serde(rename = "no-std")]
    no_std: Option<bool>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct SchemaConfig {
    out: PathBuf,
    /// JSON Schema dialect (`"7"` or `"2020-12"`).
    draft: Option<String>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct GlueConfig {
    out: PathBuf,
    #[serde(rename = "plain-root")]
    plain_root: Option<String>,
    #[serde(rename = "proto-root")]
    proto_root: Option<String>,
    /// Emit `#![no_std]` + `alloc` compatible paths instead of `::std::*`.
    #[serde(rename = "no-std")]
    no_std: Option<bool>,
}

#[derive(Debug, Clone, Copy)]
enum Emitter {
    Rust,
    Proto,
    Schema,
    Glue,
}

struct PathOverrides {
    root: Option<PathBuf>,
    out: Option<PathBuf>,
    released: Option<PathBuf>,
    plain_root: Option<String>,
    proto_root: Option<String>,
    serde: Option<bool>,
    no_std: Option<bool>,
    schema_draft: Option<emit::json_schema::JsonSchemaDraft>,
}

struct ResolvedPaths {
    root: PathBuf,
    out: PathBuf,
    released: Option<PathBuf>,
    plain_root: Option<String>,
    proto_root: Option<String>,
    serde: bool,
    no_std: bool,
    schema_draft: emit::json_schema::JsonSchemaDraft,
}

fn load_config_file(config_path: &Path) -> Result<(ConfigFile, PathBuf)> {
    let text = fs::read_to_string(config_path)
        .with_context(|| format!("failed to read configuration {}", config_path.display()))?;
    if text.starts_with('\u{feff}') {
        return Err(op_error(
            codes::PARSE_ERROR,
            format!(
                "invalid configuration {}: file starts with a UTF-8 byte-order mark (BOM); save \
                 the configuration as UTF-8 without a BOM",
                config_path.display()
            ),
            Some(config_path.display().to_string()),
        ));
    }
    let config: ConfigFile = toml::from_str(&text).map_err(|err| {
        op_error(
            codes::PARSE_ERROR,
            format!("invalid configuration {}: {err}", config_path.display()),
            Some(config_path.display().to_string()),
        )
    })?;
    if config.version != 1 {
        return Err(op_error(
            codes::PARSE_ERROR,
            format!(
                "configuration {} has unsupported version {}; expected 1",
                config_path.display(),
                config.version
            ),
            Some(config_path.display().to_string()),
        ));
    }
    if config.project.is_empty() {
        return Err(op_error(
            codes::PARSE_ERROR,
            format!("configuration {} has no projects", config_path.display()),
            Some(config_path.display().to_string()),
        ));
    }
    validate_configured_paths(&config, config_path)?;
    let base = config_path
        .parent()
        .filter(|path| !path.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .to_path_buf();
    Ok((config, base))
}

fn resolve_config_path(base: &Path, path: &Path) -> PathBuf {
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base.join(path)
    }
}

fn resolve_config_path_opt(
    config_args: &ConfigArgs,
    needs_config: bool,
) -> Result<Option<PathBuf>> {
    if let Some(path) = &config_args.config {
        return Ok(Some(path.clone()));
    }
    if config_args.no_config {
        if config_args.project.is_some() {
            return Err(op_error(
                codes::CLI_USAGE,
                "`--project` requires `--config` (discovery disabled by `--no-config`)",
                None,
            ));
        }
        return Ok(None);
    }
    if !needs_config && config_args.project.is_none() {
        return Ok(None);
    }
    if let Some(found) = workflow::discover_config() {
        return Ok(Some(found));
    }
    if config_args.project.is_some() {
        return Err(op_error(
            codes::CLI_USAGE,
            "`--project` requires `--config` (no `witforge.toml` found in this directory or above)",
            None,
        ));
    }
    Ok(None)
}

#[allow(clippy::too_many_lines)]
fn resolve_paths(
    emitter: Emitter,
    config_args: &ConfigArgs,
    overrides: PathOverrides,
) -> Result<ResolvedPaths> {
    let PathOverrides {
        root,
        out,
        released,
        plain_root,
        proto_root,
        serde,
        no_std,
        schema_draft,
    } = overrides;
    let cli_complete = root.is_some()
        && out.is_some()
        && (!matches!(emitter, Emitter::Glue) || (plain_root.is_some() && proto_root.is_some()));
    let effective_config = resolve_config_path_opt(config_args, !cli_complete)?;
    let Some(config_path) = effective_config.as_deref() else {
        if config_args.project.is_some() {
            return Err(op_error(
                codes::CLI_USAGE,
                "`--project` requires `--config`",
                None,
            ));
        }
        let Some(root) = root else {
            return Err(op_error(
                codes::CLI_USAGE,
                "missing WIT_ROOT (or use --config)",
                None,
            ));
        };
        let Some(out) = out else {
            return Err(op_error(
                codes::CLI_USAGE,
                "missing --out (or use --config)",
                None,
            ));
        };
        if matches!(emitter, Emitter::Glue) && (plain_root.is_none() || proto_root.is_none()) {
            return Err(op_error(
                codes::CLI_USAGE,
                "glue requires --plain-root and --proto-root (or use --config)",
                None,
            ));
        }
        return Ok(ResolvedPaths {
            root,
            out,
            released,
            plain_root,
            proto_root,
            serde: serde.unwrap_or(false),
            no_std: no_std.unwrap_or(false),
            schema_draft: schema_draft.unwrap_or_default(),
        });
    };

    let (config, base) = load_config_file(config_path)?;
    let name = match &config_args.project {
        Some(name) => name.clone(),
        None if config.project.len() == 1 => config
            .project
            .keys()
            .next()
            .expect("checked one project")
            .clone(),
        None => {
            return Err(op_error(
                codes::CLI_USAGE,
                format!(
                    "configuration {} has multiple projects; pass --project",
                    config_path.display()
                ),
                Some(config_path.display().to_string()),
            ));
        }
    };
    let Some(project) = config.project.get(&name).cloned() else {
        return Err(op_error(
            codes::CLI_USAGE,
            format!("configuration has no project `{name}`"),
            Some(config_path.display().to_string()),
        ));
    };
    let from_config = |path: &Path| resolve_config_path(&base, path);
    validate_configured_outputs(&config, &name, emitter, out.as_deref(), &from_config)?;
    validate_configured_module_roots(&config)?;
    let root = root.unwrap_or_else(|| from_config(&project.root));
    let out = out.or_else(|| match emitter {
        Emitter::Rust => project.rust.as_ref().map(|config| from_config(&config.out)),
        Emitter::Proto => project.proto.map(|config| from_config(&config.out)),
        Emitter::Schema => project
            .schema
            .as_ref()
            .map(|config| from_config(&config.out)),
        Emitter::Glue => project.glue.as_ref().map(|config| from_config(&config.out)),
    });
    let Some(out) = out else {
        return Err(op_error(
            codes::CLI_USAGE,
            "missing --out (and no matching output section in the configuration)",
            Some(config_path.display().to_string()),
        ));
    };
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
        return Err(op_error(
            codes::CLI_USAGE,
            "glue requires --plain-root and --proto-root (or a configuration section)",
            Some(config_path.display().to_string()),
        ));
    }
    let serde = serde
        .or_else(|| project.rust.as_ref().and_then(|config| config.serde))
        .unwrap_or(false);
    let no_std = no_std
        .or_else(|| match emitter {
            Emitter::Rust => project.rust.as_ref().and_then(|config| config.no_std),
            Emitter::Glue => project.glue.as_ref().and_then(|config| config.no_std),
            Emitter::Proto | Emitter::Schema => None,
        })
        .unwrap_or(false);
    let config_schema_draft = match project
        .schema
        .as_ref()
        .and_then(|config| config.draft.as_deref())
    {
        Some(raw) => Some(parse_schema_draft(
            raw,
            &format!("project.{name}.schema.draft"),
            Some(config_path.display().to_string()),
        )?),
        None => None,
    };
    let schema_draft = schema_draft.or(config_schema_draft).unwrap_or_default();
    Ok(ResolvedPaths {
        root,
        out,
        released,
        plain_root,
        proto_root,
        serde,
        no_std,
        schema_draft,
    })
}

fn validate_configured_outputs(
    config: &ConfigFile,
    selected_project: &str,
    selected_emitter: Emitter,
    out_override: Option<&Path>,
    from_config: &impl Fn(&Path) -> PathBuf,
) -> Result<()> {
    let mut configured: Vec<(String, PathBuf)> = Vec::new();
    let mut saw_selected = false;
    for (proj_name, proj) in &config.project {
        for (kind, slot, path) in [
            (
                Emitter::Rust,
                "rust",
                proj.rust.as_ref().map(|c| c.out.as_path()),
            ),
            (
                Emitter::Proto,
                "proto",
                proj.proto.as_ref().map(|c| c.out.as_path()),
            ),
            (
                Emitter::Schema,
                "schema",
                proj.schema.as_ref().map(|c| c.out.as_path()),
            ),
            (
                Emitter::Glue,
                "glue",
                proj.glue.as_ref().map(|c| c.out.as_path()),
            ),
        ] {
            let is_selected = proj_name == selected_project
                && std::mem::discriminant(&kind) == std::mem::discriminant(&selected_emitter);
            if is_selected && let Some(override_out) = out_override {
                configured.push(("--out".to_string(), override_out.to_path_buf()));
                saw_selected = true;
            } else if let Some(out_path) = path {
                configured.push((
                    format!("project.{proj_name}.{slot}.out"),
                    from_config(out_path),
                ));
            }
        }
    }
    if !saw_selected && let Some(override_out) = out_override {
        configured.push(("--out".to_string(), override_out.to_path_buf()));
    }
    let refs: Vec<(&str, &Path)> = configured
        .iter()
        .map(|(label, path)| (label.as_str(), path.as_path()))
        .collect();
    emit::output::ensure_distinct_output_roots(&refs)
}

fn validate_configured_paths(config: &ConfigFile, config_path: &Path) -> Result<()> {
    for (proj_name, proj) in &config.project {
        check_config_path(
            &format!("project.{proj_name}.root"),
            &proj.root,
            config_path,
        )?;
        if let Some(released) = &proj.released {
            check_config_path(
                &format!("project.{proj_name}.released"),
                released,
                config_path,
            )?;
        }
        if let Some(rust) = &proj.rust {
            check_config_path(
                &format!("project.{proj_name}.rust.out"),
                &rust.out,
                config_path,
            )?;
        }
        if let Some(proto) = &proj.proto {
            check_config_path(
                &format!("project.{proj_name}.proto.out"),
                &proto.out,
                config_path,
            )?;
        }
        if let Some(schema) = &proj.schema {
            check_config_path(
                &format!("project.{proj_name}.schema.out"),
                &schema.out,
                config_path,
            )?;
            if let Some(draft) = &schema.draft {
                parse_schema_draft(
                    draft,
                    &format!("project.{proj_name}.schema.draft"),
                    Some(config_path.display().to_string()),
                )?;
            }
        }
        if let Some(glue) = &proj.glue {
            check_config_path(
                &format!("project.{proj_name}.glue.out"),
                &glue.out,
                config_path,
            )?;
        }
    }
    Ok(())
}

fn check_config_path(label: &str, path: &Path, config_path: &Path) -> Result<()> {
    let raw = path.to_string_lossy();
    if raw.starts_with('~') {
        return Err(op_error(
            codes::GUARD_REFUSED,
            format!(
                "invalid {label} `{raw}`: configuration paths are not shell-expanded (`~` is not \
                 expanded); use a path relative to the configuration file or an absolute path"
            ),
            Some(config_path.display().to_string()),
        ));
    }
    if raw.contains('$') || contains_windows_env_var(&raw) {
        return Err(op_error(
            codes::GUARD_REFUSED,
            format!(
                "invalid {label} `{raw}`: configuration paths are not shell-expanded (environment \
                 variables are not expanded); use a path relative to the configuration file or an \
                 absolute path"
            ),
            Some(config_path.display().to_string()),
        ));
    }
    Ok(())
}

fn contains_windows_env_var(text: &str) -> bool {
    let bytes = text.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && let Some(end_offset) = bytes[i + 1..].iter().position(|&b| b == b'%')
        {
            let name = &bytes[i + 1..i + 1 + end_offset];
            if !name.is_empty()
                && (name[0].is_ascii_alphabetic() || name[0] == b'_')
                && name.iter().all(|b| b.is_ascii_alphanumeric() || *b == b'_')
            {
                return true;
            }
            i += 1 + end_offset + 1;
            continue;
        }
        i += 1;
    }
    false
}

fn parse_schema_draft(
    raw: &str,
    label: &str,
    path: Option<String>,
) -> Result<emit::json_schema::JsonSchemaDraft> {
    match raw {
        "7" => Ok(emit::json_schema::JsonSchemaDraft::Draft7),
        "2020-12" => Ok(emit::json_schema::JsonSchemaDraft::Draft202012),
        _ => Err(op_error(
            codes::CLI_USAGE,
            format!("invalid {label} `{raw}`; expected `7` or `2020-12`"),
            path,
        )),
    }
}

fn validate_configured_module_roots(config: &ConfigFile) -> Result<()> {
    for (proj_name, proj) in &config.project {
        let Some(glue) = &proj.glue else {
            continue;
        };
        if let Some(plain_root) = &glue.plain_root {
            emit::glue::validate_module_root(plain_root)
                .with_context(|| format!("invalid project.{proj_name}.glue.plain-root"))?;
        }
        if let Some(proto_root) = &glue.proto_root {
            emit::glue::validate_module_root(proto_root)
                .with_context(|| format!("invalid project.{proj_name}.glue.proto-root"))?;
        }
    }
    Ok(())
}

/// Extract `--format json|sarif` from raw argv when `clap` fails before parsing completes.
fn parse_raw_format(args: impl IntoIterator<Item = std::ffi::OsString>) -> Option<Format> {
    let mut format = None;
    let mut iter = args.into_iter();
    while let Some(arg) = iter.next() {
        let Some(text) = arg.to_str() else {
            continue;
        };
        if text == "--" {
            break;
        }
        if let Some(value) = text.strip_prefix("--format=") {
            format = match value {
                "json" => Some(Format::Json),
                "sarif" => Some(Format::Sarif),
                "human" => None,
                _ => format,
            };
        } else if text == "--format"
            && let Some(next) = iter.next()
            && let Some(value) = next.to_str()
        {
            format = match value {
                "json" => Some(Format::Json),
                "sarif" => Some(Format::Sarif),
                "human" => None,
                _ => format,
            };
        }
    }
    format
}

/// Classify an error that did not attach explicit structured diagnostics.
fn classify_fallback_error(error: &anyhow::Error) -> Diagnostic {
    if error
        .chain()
        .any(<dyn std::error::Error>::is::<std::io::Error>)
    {
        return Diagnostic::error(codes::IO_ERROR, format!("{error:#}"));
    }
    if error
        .chain()
        .any(<dyn std::error::Error>::is::<toml::de::Error>)
    {
        return Diagnostic::error(codes::PARSE_ERROR, format!("{error:#}"));
    }
    Diagnostic::error(codes::INTERNAL_ERROR, format!("{error:#}"))
}

/// Exit codes: `0` success; `1` a contract, lockfile, or generated-output violation; `2` a
/// usage, I/O, parse, released-directory, or internal error.
fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(err) => {
            let code = u8::try_from(err.exit_code()).unwrap_or(2);
            if err.use_stderr() {
                if let Some(format) = parse_raw_format(std::env::args_os().skip(1)) {
                    let mut reporter = Reporter::new(format, false);
                    reporter.push(Diagnostic::error(
                        codes::CLI_USAGE,
                        err.to_string().trim_end().to_string(),
                    ));
                    if let Err(write_err) = reporter.finish() {
                        diagnostics::write_stderr_line(&format!("witforge: {write_err:#}"));
                    }
                    return ExitCode::from(code);
                }
                diagnostics::write_stderr_str(&err.to_string());
            } else if let Err(write_err) = diagnostics::write_stdout_str(&err.to_string()) {
                diagnostics::write_stderr_line(&format!("witforge: {write_err:#}"));
                return ExitCode::from(2);
            }
            return ExitCode::from(code);
        }
    };
    match run(cli) {
        Ok(code) => code,
        Err(err) => {
            diagnostics::write_stderr_line(&format!("witforge: {err:#}"));
            ExitCode::from(if is_contract_error(&err) { 1 } else { 2 })
        }
    }
}

fn run(cli: Cli) -> Result<ExitCode> {
    let mut reporter = Reporter::new(cli.format, cli.quiet);
    let result = match cli.command {
        Command::Check(options) => check_command(&options, &mut reporter),
        Command::Lock(options) => lock(&options, &mut reporter),
        Command::Rust(options) => rust(&options, &mut reporter),
        Command::Proto(options) => proto(&options, &mut reporter),
        Command::Schema(options) => schema(&options, &mut reporter),
        Command::Glue(options) => glue(&options, &mut reporter),
        Command::Generate(options) => generate_command(&options, &mut reporter),
        Command::Init(options) => init_command(&options, &mut reporter),
        Command::NewVersion(options) => new_version_command(&options, &mut reporter),
        Command::Release(options) => release_command(&options, &mut reporter),
        Command::Retire(options) => retire_command(&options, &mut reporter),
        Command::Diff(options) => diff_command(&options, &mut reporter),
        Command::Inspect(options) => inspect_command(&options, &mut reporter),
        Command::Freeze(options) => freeze_command(&options, &mut reporter),
        Command::Completions(options) => Ok(completions_command(&options, &mut reporter)),
        Command::Explain(options) => explain_command(&options, &mut reporter),
    };
    match result {
        Ok(code) => {
            reporter.finish()?;
            Ok(code)
        }
        Err(error) => {
            if reporter.human() || reporter.is_suppressed() {
                return Err(error);
            }
            if let Some(diagnostics) = error_located_diagnostics(&error) {
                for diagnostic in diagnostics {
                    reporter.push_located(diagnostic.clone());
                }
            } else if let Some(diagnostics) = error_diagnostics(&error) {
                for diagnostic in diagnostics {
                    reporter.push(diagnostic.clone());
                }
            } else {
                reporter.push(classify_fallback_error(&error));
            }
            reporter.finish()?;
            Ok(ExitCode::from(if is_contract_error(&error) {
                1
            } else {
                2
            }))
        }
    }
}

#[allow(clippy::too_many_lines)]
fn check_command(options: &CheckOptions, reporter: &mut Reporter) -> Result<ExitCode> {
    if let Some(dir) = &options.dir {
        if options.config.config.is_some() || options.config.project.is_some() {
            return Err(op_error(
                codes::CLI_USAGE,
                "`--config` and `--project` cannot be combined with a positional PATH on `check`",
                None,
            ));
        }
        let normalized_dir: PathBuf = dir.components().collect();
        let link_meta = fs::symlink_metadata(&normalized_dir)
            .with_context(|| format!("failed to read version directory {}", dir.display()))?;
        if link_meta.file_type().is_symlink() {
            // A WIT root may be a symlink, but a symlinked version directory or dangling symlink
            // is refused with WF9004.
            let is_wit_root = fs::metadata(&normalized_dir)
                .ok()
                .filter(fs::Metadata::is_dir)
                .and_then(|_| contract_workflow::classify_check_target(&normalized_dir).ok())
                == Some(contract_workflow::CheckTargetKind::WitRoot);
            if !is_wit_root {
                ensure_version_directory(&normalized_dir)?;
            }
        } else if !link_meta.is_dir() {
            ensure_version_directory(&normalized_dir)?;
        }
        return match contract_workflow::classify_check_target(dir)? {
            contract_workflow::CheckTargetKind::VersionDirectory => {
                let single = Options {
                    dir: dir.clone(),
                    lock: options.lock.clone(),
                    against: options.against.clone(),
                    first: options.first,
                    released: options.released.clone(),
                };
                check_single(&single, reporter)
            }
            contract_workflow::CheckTargetKind::WitRoot => {
                if options.lock.is_some() || options.against.is_some() || options.first {
                    return Err(op_error(
                        codes::CLI_USAGE,
                        "`--lock`, `--first`, and `--against` apply only to a single version \
                         directory, not a WIT root",
                        Some(dir.display().to_string()),
                    ));
                }
                check_wit_root(dir, options.released.as_deref(), reporter)
            }
            contract_workflow::CheckTargetKind::Both => Err(op_error(
                codes::CLI_USAGE,
                format!(
                    "`{}` contains both WIT source/lockfile and `v<major>.<minor>` version \
                     directories; expected either a single version directory or a WIT root",
                    dir.display()
                ),
                Some(dir.display().to_string()),
            )),
            contract_workflow::CheckTargetKind::Neither => {
                // Still check for malformed version-like entries (`v00.1`, `v0.1.1`) or symlinks.
                ensure_version_directory(dir)?;
                if let Err(err) = discover_version_directories(dir)
                    && !format!("{err:#}").contains("no v<major>.<minor> version directories")
                {
                    return Err(err);
                }
                Err(op_error(
                    codes::IO_ERROR,
                    format!(
                        "`{}` contains neither `.wit` files nor `v<major>.<minor>` version \
                         directories; expected a version directory or a WIT root",
                        dir.display()
                    ),
                    Some(dir.display().to_string()),
                ))
            }
            _ => Err(op_error(
                codes::INTERNAL_ERROR,
                "unrecognized check target classification",
                Some(dir.display().to_string()),
            )),
        };
    }

    if options.lock.is_some() || options.against.is_some() || options.first {
        return Err(op_error(
            codes::CLI_USAGE,
            "`--lock`, `--first`, and `--against` require a positional version directory",
            None,
        ));
    }
    let Some(config_path) = resolve_config_path_opt(&options.config, true)? else {
        return Err(op_error(
            codes::CLI_USAGE,
            "missing PATH (pass a version directory, a WIT root, or use `witforge.toml`)",
            None,
        ));
    };
    let (config, base) = load_config_file(&config_path)?;
    let selected: Vec<(&String, &ProjectConfig)> = if let Some(name) = &options.config.project {
        let Some(project) = config.project.get(name) else {
            return Err(op_error(
                codes::CLI_USAGE,
                format!("configuration has no project `{name}`"),
                Some(config_path.display().to_string()),
            ));
        };
        vec![(name, project)]
    } else {
        config.project.iter().collect()
    };
    if selected.len() > 1 && options.released.is_some() {
        return Err(op_error(
            codes::CLI_USAGE,
            "`--released` cannot be used with multiple projects in `witforge.toml`; pass \
             `--project <NAME>` or configure `released` per project",
            Some(config_path.display().to_string()),
        ));
    }
    let mut failed = false;
    for (_, project) in selected {
        let root = resolve_config_path(&base, &project.root);
        let released = options.released.clone().or_else(|| {
            project
                .released
                .as_deref()
                .map(|p| resolve_config_path(&base, p))
        });
        let code = check_wit_root(&root, released.as_deref(), reporter)?;
        if code != ExitCode::SUCCESS {
            failed = true;
        }
    }
    Ok(if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
}

fn contract_options(options: &Options) -> contract_workflow::VersionOptions {
    let baseline = if let Some(path) = &options.against {
        contract_workflow::Baseline::Against(path.clone())
    } else if options.first {
        contract_workflow::Baseline::First
    } else {
        contract_workflow::Baseline::Default
    };
    let mut result =
        contract_workflow::VersionOptions::new(options.dir.clone()).with_baseline(baseline);
    if let Some(path) = &options.lock {
        result = result.with_lockfile(path.clone());
    }
    if let Some(path) = &options.released {
        result = result.with_released(path.clone());
    }
    result
}

fn render_workflow_report(report: &contract_workflow::Report, reporter: &mut Reporter) -> ExitCode {
    for finding in &report.findings {
        if reporter.human() {
            match finding.kind {
                contract_workflow::FindingKind::Note => {
                    if let Some(diagnostic) = &finding.diagnostic {
                        reporter.note_located(&finding.human, diagnostic.clone());
                    }
                }
                contract_workflow::FindingKind::Error
                | contract_workflow::FindingKind::SubsetError
                | contract_workflow::FindingKind::HumanOnlyError => {
                    diagnostics::write_stderr_line(&finding.human);
                }
                _ => diagnostics::write_stderr_line(&finding.human),
            }
        } else if let Some(diagnostic) = &finding.diagnostic {
            reporter.push_located(diagnostic.clone());
        }
    }
    if report.is_ok() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn check_wit_root(
    wit_root: &Path,
    explicit_released: Option<&Path>,
    reporter: &mut Reporter,
) -> Result<ExitCode> {
    let report = contract_workflow::check_root(wit_root, explicit_released)?;
    Ok(render_workflow_report(&report, reporter))
}

fn report_error_to_reporter(error: &anyhow::Error, reporter: &mut Reporter) {
    if reporter.human() {
        diagnostics::write_stderr_line(&format!("error: {error:#}"));
    } else if let Some(diagnostics) = error_located_diagnostics(error) {
        for diagnostic in diagnostics {
            reporter.push_located(diagnostic.clone());
        }
    } else if let Some(diagnostics) = error_diagnostics(error) {
        for diagnostic in diagnostics {
            reporter.push(diagnostic.clone());
        }
    } else {
        reporter.push(classify_fallback_error(error));
    }
}

fn check_single(options: &Options, reporter: &mut Reporter) -> Result<ExitCode> {
    let report = contract_workflow::check(&contract_options(options))?;
    Ok(render_workflow_report(&report, reporter))
}

fn lock(options: &Options, reporter: &mut Reporter) -> Result<ExitCode> {
    let report = contract_workflow::lock(&contract_options(options))?;
    if report.is_ok() {
        report.write()?;
    }
    Ok(render_workflow_report(&report.report, reporter))
}

fn guard_output_root(paths: &ResolvedPaths) -> Result<()> {
    // Generation reproduces released versions without modifying them, so the manifest only has
    // to be valid. The output root must stay outside both the WIT root and every released
    // directory before any file is inspected or created.
    let manifest = load_root_released_manifest(&paths.root, paths.released.as_deref())?;
    let manifest_dir = match paths.released.as_deref() {
        Some(path) => path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new(".")),
        None => &paths.root,
    };
    emit::output::ensure_outside_wit_root(&paths.root, &paths.out)?;
    emit::output::ensure_outside_released(manifest_dir, &manifest, &paths.out)
}

fn run_generator(
    files: &emit::output::GeneratedFiles,
    out: &Path,
    check: bool,
    show_diff: bool,
    regen_command: &str,
    reporter: &mut Reporter,
) -> Result<ExitCode> {
    if check {
        let findings = files.check_with_command(out, regen_command)?;
        if show_diff && reporter.human() && !findings.is_empty() {
            let diff_text = files.unified_diff(out)?;
            if !diff_text.is_empty() {
                reporter.write_stdout_line(diff_text.trim_end_matches('\n'));
            }
        }
        return Ok(check_findings(&findings, reporter));
    }
    let diff_text = if show_diff && reporter.human() {
        Some(files.unified_diff(out)?)
    } else {
        None
    };
    files.write(out)?;
    if let Some(text) = diff_text
        && !text.is_empty()
    {
        reporter.write_stdout_line(text.trim_end_matches('\n'));
    }
    Ok(ExitCode::SUCCESS)
}

#[allow(clippy::too_many_arguments)]
fn regenerate_command(
    verb: &str,
    config: &ConfigArgs,
    root: Option<&Path>,
    out: Option<&Path>,
    released: Option<&Path>,
    plain_root: Option<&str>,
    proto_root: Option<&str>,
    serde_flag: Option<bool>,
    no_std_flag: Option<bool>,
    schema_draft: Option<&str>,
) -> String {
    let mut command = format!("witforge {verb}");
    if let Some(root) = root {
        let _ = write!(command, " {}", shell_quote(root));
    }
    if let Some(out) = out {
        let _ = write!(command, " --out {}", shell_quote(out));
    }
    if let Some(plain_root) = plain_root {
        let _ = write!(
            command,
            " --plain-root {}",
            shell_quote(Path::new(plain_root))
        );
    }
    if let Some(proto_root) = proto_root {
        let _ = write!(
            command,
            " --proto-root {}",
            shell_quote(Path::new(proto_root))
        );
    }
    match serde_flag {
        Some(true) => command.push_str(" --serde"),
        Some(false) => command.push_str(" --no-serde"),
        None => {}
    }
    match no_std_flag {
        Some(true) => command.push_str(" --no-std"),
        Some(false) => command.push_str(" --std"),
        None => {}
    }
    if let Some(draft) = schema_draft {
        let _ = write!(command, " --draft {}", shell_quote(Path::new(draft)));
    }
    if let Some(released) = released {
        let _ = write!(command, " --released {}", shell_quote(released));
    }
    if let Some(cfg) = &config.config {
        let _ = write!(command, " --config {}", shell_quote(cfg));
    }
    if let Some(project) = &config.project {
        let _ = write!(command, " --project {}", shell_quote(Path::new(project)));
    }
    command
}

fn rust(options: &RustOptions, reporter: &mut Reporter) -> Result<ExitCode> {
    let serde_override = if options.serde {
        Some(true)
    } else if options.no_serde {
        Some(false)
    } else {
        None
    };
    let no_std_override = if options.no_std {
        Some(true)
    } else if options.std_mode {
        Some(false)
    } else {
        None
    };
    let paths = resolve_paths(
        Emitter::Rust,
        &options.config,
        PathOverrides {
            root: options.root.clone(),
            out: options.out.clone(),
            released: options.released.clone(),
            plain_root: None,
            proto_root: None,
            serde: serde_override,
            no_std: no_std_override,
            schema_draft: None,
        },
    )?;
    guard_output_root(&paths)?;
    let rust_opts = emit::rust::RustOptions::new()
        .with_serde(paths.serde)
        .with_no_std(paths.no_std);
    let files = emit::rust::plan_with_options(&paths.root, paths.released.as_deref(), &rust_opts)?;
    let regen = regenerate_command(
        "rust",
        &options.config,
        options.root.as_deref(),
        options.out.as_deref(),
        options.released.as_deref(),
        None,
        None,
        serde_override,
        no_std_override,
        None,
    );
    run_generator(
        &files,
        &paths.out,
        options.check,
        options.diff,
        &regen,
        reporter,
    )
}

fn proto(options: &ProtoOptions, reporter: &mut Reporter) -> Result<ExitCode> {
    let paths = resolve_paths(
        Emitter::Proto,
        &options.config,
        PathOverrides {
            root: options.root.clone(),
            out: options.out.clone(),
            released: options.released.clone(),
            plain_root: None,
            proto_root: None,
            serde: None,
            no_std: None,
            schema_draft: None,
        },
    )?;
    guard_output_root(&paths)?;
    let files = emit::proto::plan_with_released(&paths.root, paths.released.as_deref())?;
    let regen = regenerate_command(
        "proto",
        &options.config,
        options.root.as_deref(),
        options.out.as_deref(),
        options.released.as_deref(),
        None,
        None,
        None,
        None,
        None,
    );
    run_generator(
        &files,
        &paths.out,
        options.check,
        options.diff,
        &regen,
        reporter,
    )
}

fn schema(options: &SchemaOptions, reporter: &mut Reporter) -> Result<ExitCode> {
    let draft_override = options
        .draft
        .as_deref()
        .map(|raw| parse_schema_draft(raw, "`--draft`", None))
        .transpose()?;
    let paths = resolve_paths(
        Emitter::Schema,
        &options.config,
        PathOverrides {
            root: options.root.clone(),
            out: options.out.clone(),
            released: options.released.clone(),
            plain_root: None,
            proto_root: None,
            serde: None,
            no_std: None,
            schema_draft: draft_override,
        },
    )?;
    guard_output_root(&paths)?;
    let schema_options = emit::json_schema::SchemaOptions::new().with_draft(paths.schema_draft);
    let files = emit::json_schema::plan_with_options(
        &paths.root,
        paths.released.as_deref(),
        &schema_options,
    )?;
    let regen = regenerate_command(
        "schema",
        &options.config,
        options.root.as_deref(),
        options.out.as_deref(),
        options.released.as_deref(),
        None,
        None,
        None,
        None,
        options.draft.as_deref(),
    );
    run_generator(
        &files,
        &paths.out,
        options.check,
        options.diff,
        &regen,
        reporter,
    )
}

fn glue(options: &GlueOptions, reporter: &mut Reporter) -> Result<ExitCode> {
    let no_std_override = if options.no_std {
        Some(true)
    } else if options.std_mode {
        Some(false)
    } else {
        None
    };
    let paths = resolve_paths(
        Emitter::Glue,
        &options.config,
        PathOverrides {
            root: options.root.clone(),
            out: options.out.clone(),
            released: options.released.clone(),
            plain_root: options.plain_root.clone(),
            proto_root: options.proto_root.clone(),
            serde: None,
            no_std: no_std_override,
            schema_draft: None,
        },
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
    // stay frozen. As for the other generators, the manifest is validated and any `retired`
    // entries are excluded from output.
    guard_output_root(&paths)?;
    let glue_opts = emit::glue::GlueOptions::new().with_no_std(paths.no_std);
    let files = emit::glue::plan_with_options(
        &paths.root,
        plain_root,
        proto_root,
        paths.released.as_deref(),
        &glue_opts,
    )?;
    let regen = regenerate_command(
        "glue",
        &options.config,
        options.root.as_deref(),
        options.out.as_deref(),
        options.released.as_deref(),
        options.plain_root.as_deref(),
        options.proto_root.as_deref(),
        None,
        no_std_override,
        None,
    );
    run_generator(
        &files,
        &paths.out,
        options.check,
        options.diff,
        &regen,
        reporter,
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(clippy::struct_excessive_bools)]
struct CheckViews {
    wire: bool,
    source: bool,
    api: bool,
    decode: bool,
}

impl CheckViews {
    /// Whether this is the default `--check` view set (`wire,source,api`; excludes `decode`).
    fn is_default_set(self) -> bool {
        self.wire && self.source && self.api && !self.decode
    }

    fn matches(self, change: &diff::Change, verdict: diff::DecodeVerdict) -> bool {
        (self.wire && change.wire_breaking)
            || (self.source && change.source_breaking)
            || (self.api && change.api_breaking)
            || (self.decode && verdict.is_breaking())
    }

    fn label(self) -> String {
        let mut names = Vec::new();
        if self.wire {
            names.push("wire");
        }
        if self.source {
            names.push("source");
        }
        if self.api {
            names.push("api");
        }
        if self.decode {
            names.push("decode");
        }
        names.join(", ")
    }
}

fn parse_check_views(raw: Option<&str>) -> Result<Option<CheckViews>> {
    let Some(spec) = raw else {
        return Ok(None);
    };
    if spec.is_empty() {
        return Err(op_error(
            codes::CLI_USAGE,
            "invalid `--check` view set: expected a non-empty comma-separated list of `wire`, \
             `source`, `api`, `decode`",
            None,
        ));
    }
    let mut views = CheckViews {
        wire: false,
        source: false,
        api: false,
        decode: false,
    };
    for part in spec.split(',') {
        match part {
            "wire" => views.wire = true,
            "source" => views.source = true,
            "api" => views.api = true,
            "decode" => views.decode = true,
            "" => {
                return Err(op_error(
                    codes::CLI_USAGE,
                    format!(
                        "invalid `--check={spec}`: empty view name; expected `wire`, `source`, \
                         `api`, or `decode`"
                    ),
                    None,
                ));
            }
            other => {
                return Err(op_error(
                    codes::CLI_USAGE,
                    format!(
                        "unknown `--check` view `{other}`; expected `wire`, `source`, `api`, or \
                         `decode`"
                    ),
                    None,
                ));
            }
        }
    }
    Ok(Some(views))
}

#[allow(clippy::too_many_lines)]
fn diff_command(options: &DiffOptions, reporter: &mut Reporter) -> Result<ExitCode> {
    if options.json {
        // `--json` is the diff report; it takes precedence over the diagnostic envelope even on
        // error, so a non-human `--format` never prints a diagnostic envelope instead of `--json`.
        reporter.suppress();
    }
    let check_views = parse_check_views(options.check.as_deref())?;
    let full = diff::compare_full(&options.old, &options.new)?;
    let analysis = &full.analysis;
    let omissions = &full.conversion_omissions;
    let decode_verdicts = &full.decode_verdicts;
    let decode_summary = full.decode_summary;
    let locations = &full.locations;
    let report = &analysis.report;
    let cross_major = report.old.version.major != report.new.version.major;
    if options.json {
        let rendered = serde_json::to_string_pretty(&diff_json(&full))
            .context("failed to render the diff as JSON")?;
        reporter.write_stdout_line(&rendered);
    } else if reporter.human() {
        let gated_breaking = check_views.map(|views| {
            report
                .changes
                .iter()
                .zip(decode_verdicts.iter())
                .filter(|(change, verdict)| views.matches(change, **verdict))
                .count()
        });
        let rendered = render_diff(
            report,
            decode_verdicts,
            decode_summary,
            gated_breaking,
            reporter.is_quiet(),
        );
        if !rendered.is_empty() {
            reporter.write_stdout_line(rendered.trim_end_matches('\n'));
        }
    } else {
        for (change, &verdict) in report.changes.iter().zip(decode_verdicts.iter()) {
            let gated_hit = check_views.is_some_and(|views| views.matches(change, verdict));
            let (code, severity) = if change.is_breaking() || gated_hit {
                (
                    "WF3001",
                    if gated_hit {
                        Severity::Error
                    } else {
                        Severity::Warning
                    },
                )
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
            let mut diag = Diagnostic::new(code, severity, message).with_path(change.path.clone());
            let mut located =
                LocatedDiagnostic::from_diagnostic(diag.clone()).with_logical(change.path.clone());
            if let Some((file, pos)) = locations.get(&change.path) {
                diag = diag.with_position(*pos);
                located = LocatedDiagnostic::from_diagnostic(diag)
                    .with_file(file.clone())
                    .with_logical(change.path.clone());
            }
            reporter.push_located(located);
        }
    }
    if !options.json {
        for impact in &analysis.conversion_impacts {
            let human = format!(
                "note: new-to-old conversion for `{}` ({} -> {}) always fails: missing required \
                 member `{}`",
                impact.type_path, impact.from_version, impact.to_version, impact.missing_path
            );
            let message = format!(
                "guaranteed new-to-old conversion failure for `{}` ({} -> {}): missing required \
                 member `{}`",
                impact.type_path, impact.from_version, impact.to_version, impact.missing_path
            );
            let loc = locations
                .get(&impact.missing_path)
                .or_else(|| locations.get(&impact.type_path));
            reporter.note_located(
                &human,
                located_note("WF3003", message, impact.missing_path.clone(), loc, None),
            );
        }
        if cross_major {
            for omission in omissions {
                let human = format!(
                    "note: conversion {} -> {} for `{}` is omitted: {}",
                    omission.from_version, omission.to_version, omission.type_path, omission.reason
                );
                let message = format!(
                    "conversion {} -> {} for `{}` is omitted: {}",
                    omission.from_version, omission.to_version, omission.type_path, omission.reason
                );
                reporter.note_located(
                    &human,
                    located_note(
                        "WF3004",
                        message,
                        omission.type_path.clone(),
                        locations.get(&omission.type_path),
                        None,
                    ),
                );
            }
        } else {
            for (kind, blocker) in conversion_blockers(report) {
                let (human, message) = match kind {
                    "required-addition" => (
                        format!(
                            "note: conversion generation refuses `{}`: a required record field was \
                             added without a default; use option<T>, list<T>, map<K, V>, or a new member",
                            blocker.path
                        ),
                        format!(
                            "conversion generation refuses `{}`: a required record field was added \
                             without a default",
                            blocker.path
                        ),
                    ),
                    _ => (
                        format!(
                            "note: conversion generation refuses `{}`: the type changed; add a new \
                             member or type instead",
                            blocker.path
                        ),
                        format!(
                            "conversion generation refuses `{}`: the type changed",
                            blocker.path
                        ),
                    ),
                };
                reporter.note_located(
                    &human,
                    located_note(
                        "WF3004",
                        message,
                        blocker.path.clone(),
                        locations.get(&blocker.path),
                        None,
                    ),
                );
            }
        }
    }
    if let Some(views) = check_views {
        let gated_breaking = report
            .changes
            .iter()
            .zip(decode_verdicts.iter())
            .filter(|(change, verdict)| views.matches(change, **verdict))
            .count();
        if gated_breaking > 0 {
            if reporter.human() || reporter.is_suppressed() {
                if views.is_default_set() {
                    diagnostics::write_stderr_line(&format!(
                        "diff: {gated_breaking} breaking change(s); --check failed"
                    ));
                } else {
                    diagnostics::write_stderr_line(&format!(
                        "diff: {gated_breaking} breaking change(s) in gated view(s) ({}); \
                         --check failed",
                        views.label()
                    ));
                }
            }
            return Ok(ExitCode::from(1));
        }
    }
    Ok(ExitCode::SUCCESS)
}

fn completions_command(options: &CompletionsOptions, reporter: &mut Reporter) -> ExitCode {
    let mut cmd = Cli::command();
    let script = completions::generate(options.shell, &mut cmd);
    reporter.suppress();
    reporter.write_stdout_line(script.trim_end_matches('\n'));
    ExitCode::SUCCESS
}

fn explain_command(options: &ExplainOptions, reporter: &mut Reporter) -> Result<ExitCode> {
    let normalized = options.code.trim().to_ascii_uppercase();
    let Some(info) = witforge::diagnostic::lookup(&normalized) else {
        return Err(op_error(
            codes::CLI_USAGE,
            format!(
                "unknown diagnostic code `{}`; codes have the form `WFxxxx` (for example \
                 `WF1001`)",
                options.code
            ),
            None,
        ));
    };
    let meaning = witforge::diagnostic::explanation(info.code).unwrap_or(info.summary);
    let mut out = String::new();
    let _ = writeln!(out, "{} ({})", info.code, info.severity);
    if let Some(prior) = info.supersedes {
        let _ = writeln!(out, "Prior code: {prior}");
    }
    let _ = writeln!(out, "\n{meaning}");
    let _ = write!(out, "\nDocs: {}", witforge::diagnostic::help_uri(info.code));
    reporter.suppress();
    reporter.write_stdout_line(&out);
    Ok(ExitCode::SUCCESS)
}

#[allow(clippy::too_many_lines)]
fn inspect_command(options: &InspectOptions, reporter: &mut Reporter) -> Result<ExitCode> {
    if options.json {
        reporter.suppress();
    }
    let (target, released) = if let Some(path) = &options.path {
        if options.config.config.is_some() || options.config.project.is_some() {
            return Err(op_error(
                codes::CLI_USAGE,
                "`--config` and `--project` cannot be combined with a positional PATH on `inspect`",
                None,
            ));
        }
        (path.clone(), options.released.clone())
    } else {
        let Some(config_path) = resolve_config_path_opt(&options.config, true)? else {
            return Err(op_error(
                codes::CLI_USAGE,
                "missing PATH (pass a version directory, a WIT root, or use `witforge.toml`)",
                None,
            ));
        };
        let (config, base) = load_config_file(&config_path)?;
        let name = match &options.config.project {
            Some(name) => name.clone(),
            None if config.project.len() == 1 => config
                .project
                .keys()
                .next()
                .expect("checked one project")
                .clone(),
            None => {
                return Err(op_error(
                    codes::CLI_USAGE,
                    format!(
                        "configuration {} has multiple projects; pass --project",
                        config_path.display()
                    ),
                    Some(config_path.display().to_string()),
                ));
            }
        };
        let Some(project) = config.project.get(&name) else {
            return Err(op_error(
                codes::CLI_USAGE,
                format!("configuration has no project `{name}`"),
                Some(config_path.display().to_string()),
            ));
        };
        let root = resolve_config_path(&base, &project.root);
        let released = options.released.clone().or_else(|| {
            project
                .released
                .as_deref()
                .map(|p| resolve_config_path(&base, p))
        });
        (root, released)
    };

    let normalized_target: PathBuf = target.components().collect();
    let link_meta = fs::symlink_metadata(&normalized_target)
        .with_context(|| format!("failed to read {}", target.display()))?;
    if link_meta.file_type().is_symlink() {
        let is_wit_root = fs::metadata(&normalized_target)
            .ok()
            .filter(fs::Metadata::is_dir)
            .and_then(|_| contract_workflow::classify_check_target(&normalized_target).ok())
            == Some(contract_workflow::CheckTargetKind::WitRoot);
        if !is_wit_root {
            ensure_version_directory(&normalized_target)?;
        }
    } else if !link_meta.is_dir() {
        ensure_version_directory(&normalized_target)?;
    }

    let rendered = match contract_workflow::classify_check_target(&target)? {
        contract_workflow::CheckTargetKind::VersionDirectory => {
            if options.released.is_some() {
                return Err(op_error(
                    codes::CLI_USAGE,
                    "`--released` applies only to a WIT root, not a single version directory",
                    Some(target.display().to_string()),
                ));
            }
            if options.json {
                witforge::inspect::inspect_version_json(&target)?
            } else {
                witforge::inspect::summarize_version(&target)?
            }
        }
        contract_workflow::CheckTargetKind::WitRoot => {
            if options.json {
                witforge::inspect::inspect_root_json(&target, released.as_deref())?
            } else {
                witforge::inspect::summarize_root(&target, released.as_deref())?
            }
        }
        contract_workflow::CheckTargetKind::Both => {
            return Err(op_error(
                codes::CLI_USAGE,
                format!(
                    "`{}` contains both WIT source/lockfile and `v<major>.<minor>` version \
                     directories; expected either a single version directory or a WIT root",
                    target.display()
                ),
                Some(target.display().to_string()),
            ));
        }
        contract_workflow::CheckTargetKind::Neither => {
            ensure_version_directory(&target)?;
            if let Err(err) = discover_version_directories(&target)
                && !format!("{err:#}").contains("no v<major>.<minor> version directories")
            {
                return Err(err);
            }
            return Err(op_error(
                codes::IO_ERROR,
                format!(
                    "`{}` contains neither `.wit` files nor `v<major>.<minor>` version \
                     directories; expected a version directory or a WIT root",
                    target.display()
                ),
                Some(target.display().to_string()),
            ));
        }
        _ => {
            return Err(op_error(
                codes::INTERNAL_ERROR,
                "unrecognized inspect target classification",
                Some(target.display().to_string()),
            ));
        }
    };

    if options.json || (reporter.human() && !reporter.is_quiet()) {
        reporter.write_stdout_line(rendered.trim_end_matches('\n'));
    }
    Ok(ExitCode::SUCCESS)
}

fn freeze_command(options: &FreezeOptions, reporter: &mut Reporter) -> Result<ExitCode> {
    let targets = resolve_freeze_roots(options)?;
    let freeze_opts = witforge::freeze::FreezeOptions {
        base: options.base.clone(),
    };
    let mut failed = false;
    for (root, manifest) in targets {
        let report =
            witforge::freeze::check_root_with_manifest(&root, manifest.as_deref(), &freeze_opts)?;
        if report.is_ok() {
            if reporter.human() && !reporter.is_quiet() {
                if report.no_manifest {
                    reporter.write_stdout_line(&format!(
                        "no released manifest at {}; nothing to freeze",
                        report.manifest
                    ));
                } else {
                    reporter.write_stdout_line(&format!(
                        "release freeze check passed for {} ({} release check(s))",
                        report.root, report.checks
                    ));
                }
            }
        } else {
            failed = true;
            for violation in report.violations {
                if reporter.human() {
                    diagnostics::write_stderr_line(&format!(
                        "release freeze: {}",
                        violation.diagnostic.message
                    ));
                } else {
                    reporter.push_located(violation);
                }
            }
        }
    }
    if failed {
        Ok(ExitCode::from(1))
    } else {
        Ok(ExitCode::SUCCESS)
    }
}

fn resolve_freeze_roots(options: &FreezeOptions) -> Result<Vec<(PathBuf, Option<PathBuf>)>> {
    if let Some(root) = &options.root {
        if options.config.config.is_some() || options.config.project.is_some() {
            return Err(op_error(
                codes::CLI_USAGE,
                "`--config` and `--project` cannot be combined with a positional WIT_ROOT on `freeze`",
                None,
            ));
        }
        return Ok(vec![(root.clone(), options.released.clone())]);
    }
    let Some(config_path) = resolve_config_path_opt(&options.config, true)? else {
        return Err(op_error(
            codes::CLI_USAGE,
            "missing WIT_ROOT (or use --config)",
            None,
        ));
    };
    let (config, base) = load_config_file(&config_path)?;
    if let Some(name) = &options.config.project {
        let Some(project) = config.project.get(name) else {
            return Err(op_error(
                codes::CLI_USAGE,
                format!("configuration has no project `{name}`"),
                Some(config_path.display().to_string()),
            ));
        };
        let released = options.released.clone().or_else(|| {
            project
                .released
                .as_deref()
                .map(|p| resolve_config_path(&base, p))
        });
        return Ok(vec![(resolve_config_path(&base, &project.root), released)]);
    }
    if config.project.len() > 1 && options.released.is_some() {
        return Err(op_error(
            codes::CLI_USAGE,
            "`--released` cannot be used with multiple projects in `witforge.toml`; pass \
             `--project <NAME>` or configure `released` per project",
            Some(config_path.display().to_string()),
        ));
    }
    Ok(config
        .project
        .values()
        .map(|project| {
            let released = options.released.clone().or_else(|| {
                project
                    .released
                    .as_deref()
                    .map(|p| resolve_config_path(&base, p))
            });
            (resolve_config_path(&base, &project.root), released)
        })
        .collect())
}

#[allow(clippy::too_many_lines)]
fn generate_command(options: &GenerateOptions, reporter: &mut Reporter) -> Result<ExitCode> {
    let Some(config_path) = resolve_config_path_opt(&options.config, true)? else {
        return Err(op_error(
            codes::CLI_USAGE,
            "missing configuration (`witforge.toml` not found; pass --config)",
            None,
        ));
    };
    let (config, base) = load_config_file(&config_path)?;
    let from_config = |path: &Path| resolve_config_path(&base, path);
    // Validate distinct output roots and glue module roots across the whole configuration up front.
    let mut all_outs: Vec<(String, PathBuf)> = Vec::new();
    for (proj_name, proj) in &config.project {
        if let Some(rust) = &proj.rust {
            all_outs.push((
                format!("project.{proj_name}.rust.out"),
                from_config(&rust.out),
            ));
        }
        if let Some(proto) = &proj.proto {
            all_outs.push((
                format!("project.{proj_name}.proto.out"),
                from_config(&proto.out),
            ));
        }
        if let Some(schema) = &proj.schema {
            all_outs.push((
                format!("project.{proj_name}.schema.out"),
                from_config(&schema.out),
            ));
        }
        if let Some(glue) = &proj.glue {
            all_outs.push((
                format!("project.{proj_name}.glue.out"),
                from_config(&glue.out),
            ));
        }
    }
    let refs: Vec<(&str, &Path)> = all_outs
        .iter()
        .map(|(label, path)| (label.as_str(), path.as_path()))
        .collect();
    emit::output::ensure_distinct_output_roots(&refs)?;
    validate_configured_module_roots(&config)?;

    let selected: Vec<(String, ProjectConfig)> = if let Some(name) = &options.config.project {
        let Some(project) = config.project.get(name) else {
            return Err(op_error(
                codes::CLI_USAGE,
                format!("configuration has no project `{name}`"),
                Some(config_path.display().to_string()),
            ));
        };
        vec![(name.clone(), project.clone())]
    } else {
        config
            .project
            .iter()
            .map(|(name, proj)| (name.clone(), proj.clone()))
            .collect()
    };

    if selected.len() > 1 && options.released.is_some() {
        return Err(op_error(
            codes::CLI_USAGE,
            "`--released` cannot be used with multiple projects in `witforge.toml`; pass \
             `--project <NAME>` or configure `released` per project",
            Some(config_path.display().to_string()),
        ));
    }

    let serde_override = if options.serde {
        Some(true)
    } else if options.no_serde {
        Some(false)
    } else {
        None
    };
    let no_std_override = if options.no_std {
        Some(true)
    } else if options.std_mode {
        Some(false)
    } else {
        None
    };
    if let Some(raw) = options.draft.as_deref() {
        parse_schema_draft(raw, "`--draft`", None)?;
    }

    let mut planned: Vec<(String, Vec<Emitter>)> = Vec::with_capacity(selected.len());
    for (proj_name, proj) in &selected {
        let mut emitters = Vec::new();
        if proj.rust.is_some() {
            emitters.push(Emitter::Rust);
        }
        if proj.proto.is_some() {
            emitters.push(Emitter::Proto);
        }
        if proj.schema.is_some() {
            emitters.push(Emitter::Schema);
        }
        if proj.glue.is_some() {
            emitters.push(Emitter::Glue);
        }
        if emitters.is_empty() {
            return Err(op_error(
                codes::CLI_USAGE,
                format!(
                    "project `{proj_name}` in {} configures no generators (rust, proto, schema, \
                     or glue)",
                    config_path.display()
                ),
                Some(config_path.display().to_string()),
            ));
        }
        planned.push((proj_name.clone(), emitters));
    }

    let mut had_violation = false;
    let mut had_operational = false;
    let mut ran_any = false;
    let multi_project = config.project.len() > 1;

    for (proj_name, emitters) in planned {
        let cfg_args = ConfigArgs {
            config: Some(config_path.clone()),
            no_config: false,
            project: if multi_project {
                Some(proj_name.clone())
            } else {
                None
            },
        };
        for emitter in emitters {
            ran_any = true;
            let step = match emitter {
                Emitter::Rust => rust(
                    &RustOptions {
                        root: None,
                        out: None,
                        check: options.check,
                        diff: options.diff,
                        serde: serde_override == Some(true),
                        no_serde: serde_override == Some(false),
                        no_std: no_std_override == Some(true),
                        std_mode: no_std_override == Some(false),
                        released: options.released.clone(),
                        config: ConfigArgs {
                            config: cfg_args.config.clone(),
                            no_config: false,
                            project: cfg_args.project.clone(),
                        },
                    },
                    reporter,
                ),
                Emitter::Proto => proto(
                    &ProtoOptions {
                        root: None,
                        out: None,
                        check: options.check,
                        diff: options.diff,
                        released: options.released.clone(),
                        config: ConfigArgs {
                            config: cfg_args.config.clone(),
                            no_config: false,
                            project: cfg_args.project.clone(),
                        },
                    },
                    reporter,
                ),
                Emitter::Schema => schema(
                    &SchemaOptions {
                        root: None,
                        out: None,
                        draft: options.draft.clone(),
                        check: options.check,
                        diff: options.diff,
                        released: options.released.clone(),
                        config: ConfigArgs {
                            config: cfg_args.config.clone(),
                            no_config: false,
                            project: cfg_args.project.clone(),
                        },
                    },
                    reporter,
                ),
                Emitter::Glue => glue(
                    &GlueOptions {
                        root: None,
                        out: None,
                        plain_root: None,
                        proto_root: None,
                        no_std: no_std_override == Some(true),
                        std_mode: no_std_override == Some(false),
                        check: options.check,
                        diff: options.diff,
                        released: options.released.clone(),
                        config: ConfigArgs {
                            config: cfg_args.config.clone(),
                            no_config: false,
                            project: cfg_args.project.clone(),
                        },
                    },
                    reporter,
                ),
            };
            match step {
                Ok(code) if code == ExitCode::SUCCESS => {}
                Ok(_) => had_violation = true,
                Err(err) if is_contract_error(&err) => {
                    had_violation = true;
                    report_error_to_reporter(&err, reporter);
                }
                Err(err) => {
                    had_operational = true;
                    report_error_to_reporter(&err, reporter);
                }
            }
        }
    }

    if !ran_any {
        return Err(op_error(
            codes::CLI_USAGE,
            "configuration defines no generators to run",
            Some(config_path.display().to_string()),
        ));
    }
    if had_operational {
        Ok(ExitCode::from(2))
    } else if had_violation {
        Ok(ExitCode::from(1))
    } else {
        Ok(ExitCode::SUCCESS)
    }
}

fn init_command(options: &InitOptions, reporter: &mut Reporter) -> Result<ExitCode> {
    let target_dir = options.dir.clone().unwrap_or_else(|| PathBuf::from("."));
    let (package_id, config_path) =
        workflow::init_project(&target_dir, options.package.as_deref(), options.serde)?;
    if reporter.human() && !reporter.is_quiet() {
        reporter.write_stdout_line(&format!(
            "initialized {} for {package_id}@0.1.0 (next: `witforge check --config {}` and \
             `witforge generate --config {}`)",
            target_dir.display(),
            shell_quote(&config_path),
            shell_quote(&config_path),
        ));
    }
    Ok(ExitCode::SUCCESS)
}

fn new_version_command(options: &NewVersionOptions, reporter: &mut Reporter) -> Result<ExitCode> {
    let (wit_root, configured_released) = if let Some(root) = &options.root {
        if options.config.config.is_some() || options.config.project.is_some() {
            return Err(op_error(
                codes::CLI_USAGE,
                "`--config` and `--project` cannot be combined with a positional WIT_ROOT on `new-version`",
                None,
            ));
        }
        (root.clone(), None)
    } else {
        let Some(config_path) = resolve_config_path_opt(&options.config, true)? else {
            return Err(op_error(
                codes::CLI_USAGE,
                "missing WIT_ROOT (or use --config)",
                None,
            ));
        };
        let (config, base) = load_config_file(&config_path)?;
        let name = match &options.config.project {
            Some(name) => name.clone(),
            None if config.project.len() == 1 => config
                .project
                .keys()
                .next()
                .expect("checked one project")
                .clone(),
            None => {
                return Err(op_error(
                    codes::CLI_USAGE,
                    format!(
                        "configuration {} has multiple projects; pass --project",
                        config_path.display()
                    ),
                    Some(config_path.display().to_string()),
                ));
            }
        };
        let Some(project) = config.project.get(&name) else {
            return Err(op_error(
                codes::CLI_USAGE,
                format!("configuration has no project `{name}`"),
                Some(config_path.display().to_string()),
            ));
        };
        let root = resolve_config_path(&base, &project.root);
        let released = project
            .released
            .as_deref()
            .map(|p| resolve_config_path(&base, p));
        (root, released)
    };

    let released_override = if let Some(explicit) = options.released.as_deref() {
        Some((explicit, true))
    } else {
        configured_released
            .as_deref()
            .map(|configured| (configured, true))
    };

    let (latest_dir, target_dir, copied_lock) = workflow::create_new_version(
        &wit_root,
        options.major,
        options.from.as_deref(),
        released_override,
    )?;
    if reporter.human() && !reporter.is_quiet() {
        let next_cmd = if copied_lock {
            format!(
                "witforge lock {} --against {}",
                shell_quote(&target_dir),
                shell_quote(&latest_dir.join("wire.lock"))
            )
        } else {
            format!("witforge lock {} --first", shell_quote(&target_dir))
        };
        reporter.write_stdout_line(&format!(
            "created {} from {} (next: `{next_cmd}`)",
            target_dir.display(),
            latest_dir.display()
        ));
    }
    Ok(ExitCode::SUCCESS)
}

fn resolve_manifest_for_version_dir(
    dir: &Path,
    explicit_released: Option<&Path>,
    config_args: &ConfigArgs,
) -> Result<(PathBuf, bool)> {
    let configured_released = if config_args.config.is_some() || config_args.project.is_some() {
        let Some(config_path) = resolve_config_path_opt(config_args, true)? else {
            return Err(op_error(
                codes::CLI_USAGE,
                "`--project` requires `--config`",
                None,
            ));
        };
        let (config, base) = load_config_file(&config_path)?;
        let name = match &config_args.project {
            Some(name) => name.clone(),
            None if config.project.len() == 1 => config
                .project
                .keys()
                .next()
                .expect("checked one project")
                .clone(),
            None => {
                return Err(op_error(
                    codes::CLI_USAGE,
                    format!(
                        "configuration {} has multiple projects; pass --project",
                        config_path.display()
                    ),
                    Some(config_path.display().to_string()),
                ));
            }
        };
        let Some(project) = config.project.get(&name) else {
            return Err(op_error(
                codes::CLI_USAGE,
                format!("configuration has no project `{name}`"),
                Some(config_path.display().to_string()),
            ));
        };
        project
            .released
            .as_deref()
            .map(|p| resolve_config_path(&base, p))
    } else {
        None
    };

    if let Some(explicit) = explicit_released {
        return Ok((explicit.to_path_buf(), true));
    }
    if let Some(configured) = configured_released {
        return Ok((configured, true));
    }
    Ok((default_released_manifest(dir), false))
}

fn ensure_manifest_belongs_to_wit_root(manifest_path: &Path, wit_root: &Path) -> Result<()> {
    workflow::ensure_manifest_belongs_to_wit_root(manifest_path, wit_root)
}

#[allow(clippy::too_many_lines)]
fn release_command(options: &ReleaseOptions, reporter: &mut Reporter) -> Result<ExitCode> {
    ensure_version_directory(&options.dir)?;
    let canonical_dir = fs::canonicalize(&options.dir).with_context(|| {
        format!(
            "failed to resolve version directory {}",
            options.dir.display()
        )
    })?;
    let entry_name = canonical_dir
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .map(str::to_string)
        .filter(|name| version_directory(Path::new(name)).is_some())
        .ok_or_else(|| {
            op_error(
                codes::CLI_USAGE,
                format!(
                    "`{}` is not a `v<major>.<minor>` version directory",
                    options.dir.display()
                ),
                Some(options.dir.display().to_string()),
            )
        })?;
    let wit_root = canonical_dir.parent().ok_or_else(|| {
        op_error(
            codes::IO_ERROR,
            format!(
                "version directory `{}` has no parent WIT root",
                options.dir.display()
            ),
            Some(options.dir.display().to_string()),
        )
    })?;

    let (raw_manifest_path, _) = resolve_manifest_for_version_dir(
        &options.dir,
        options.released.as_deref(),
        &options.config,
    )?;
    let manifest_path: PathBuf = raw_manifest_path.components().collect();
    if fs::symlink_metadata(&manifest_path).is_ok_and(|meta| meta.file_type().is_symlink()) {
        return Err(op_error(
            codes::GUARD_REFUSED,
            format!(
                "released manifest `{}` is a symlink; commit a regular file",
                raw_manifest_path.display()
            ),
            Some(raw_manifest_path.display().to_string()),
        ));
    }
    ensure_manifest_belongs_to_wit_root(&manifest_path, wit_root)?;

    let manifest = if manifest_path.exists() {
        ReleasedManifest::load(&manifest_path)?
    } else {
        ReleasedManifest::default()
    };

    let already_listed = manifest.contains(&entry_name);

    // Predecessor in the same major must already be released before releasing this version.
    let predecessor = sibling_predecessor(&canonical_dir);
    if !already_listed && let Some((_, ref pred_dir)) = predecessor {
        let pred_name = pred_dir
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or_default();
        if !manifest.contains(pred_name) {
            let diag = Diagnostic::error(
                codes::LOCKFILE_ERROR,
                format!(
                    "cannot release `{entry_name}`: predecessor `{pred_name}` is not listed \
                     in {}; release `{pred_name}` first",
                    manifest_path.display()
                ),
            )
            .with_path(options.dir.display().to_string());
            bail!(ContractError::with_located_diagnostic(
                LocatedDiagnostic::from_diagnostic(diag)
                    .with_file(manifest_path.display().to_string())
            ));
        }
    }

    // Released directories must contain no symlinks (checked before `is_file()` so a dangling
    // `wire.lock` symlink reports WF9004 rather than WF9002) and must hold a real wire.lock.
    ensure_no_symlinks_in_dir(&options.dir, &entry_name)?;
    let lock_path = options.dir.join("wire.lock");
    if !lock_path.is_file() {
        return Err(op_error(
            codes::IO_ERROR,
            format!(
                "cannot release `{}`: lockfile {} does not exist",
                options.dir.display(),
                lock_path.display()
            ),
            Some(lock_path.display().to_string()),
        ));
    }

    // Validate the version directory and enclosing WIT-root invariants (package identity,
    // `@since` history, dependency-graph identity, and retirement order) before recording.
    let report = contract_workflow::check_release(&options.dir, Some(&manifest_path))?;
    let status = render_workflow_report(&report, reporter);
    if status != ExitCode::SUCCESS {
        return Ok(status);
    }

    if already_listed {
        if reporter.human() && !reporter.is_quiet() {
            reporter.write_stdout_line(&format!(
                "`{entry_name}` is already listed in {}",
                manifest_path.display()
            ));
        }
        return Ok(ExitCode::SUCCESS);
    }

    workflow::append_released_entry(&manifest_path, &entry_name)?;
    if reporter.human() && !reporter.is_quiet() {
        reporter.write_stdout_line(&format!(
            "released `{entry_name}` in {}",
            manifest_path.display()
        ));
    }
    Ok(ExitCode::SUCCESS)
}

#[allow(clippy::too_many_lines)]
fn retire_command(options: &RetireOptions, reporter: &mut Reporter) -> Result<ExitCode> {
    ensure_version_directory(&options.dir)?;
    let canonical_dir = fs::canonicalize(&options.dir).with_context(|| {
        format!(
            "failed to resolve version directory {}",
            options.dir.display()
        )
    })?;
    let entry_name = canonical_dir
        .file_name()
        .and_then(std::ffi::OsStr::to_str)
        .map(str::to_string)
        .filter(|name| version_directory(Path::new(name)).is_some())
        .ok_or_else(|| {
            op_error(
                codes::CLI_USAGE,
                format!(
                    "`{}` is not a `v<major>.<minor>` version directory",
                    options.dir.display()
                ),
                Some(options.dir.display().to_string()),
            )
        })?;
    let wit_root = canonical_dir.parent().ok_or_else(|| {
        op_error(
            codes::IO_ERROR,
            format!(
                "version directory `{}` has no parent WIT root",
                options.dir.display()
            ),
            Some(options.dir.display().to_string()),
        )
    })?;

    let (raw_manifest_path, explicit_manifest) = resolve_manifest_for_version_dir(
        &options.dir,
        options.released.as_deref(),
        &options.config,
    )?;
    let manifest_path: PathBuf = raw_manifest_path.components().collect();
    let manifest_file = raw_manifest_path.display().to_string();

    if fs::symlink_metadata(&manifest_path).is_ok_and(|meta| meta.file_type().is_symlink()) {
        return Err(op_error(
            codes::GUARD_REFUSED,
            format!("released manifest `{manifest_file}` is a symlink; commit a regular file"),
            Some(manifest_file),
        ));
    }
    if !manifest_path.exists() {
        if explicit_manifest {
            return Err(op_error(
                codes::IO_ERROR,
                format!("released manifest `{manifest_file}` does not exist"),
                Some(manifest_file),
            ));
        }
        let diag = Diagnostic::error(
            codes::LOCKFILE_ERROR,
            format!(
                "cannot retire `{entry_name}`: released manifest {manifest_file} does not exist; \
                 release `{entry_name}` first"
            ),
        )
        .with_path(manifest_file.clone());
        bail!(ContractError::with_located_diagnostic(
            LocatedDiagnostic::from_diagnostic(diag).with_file(manifest_file)
        ));
    }
    ensure_manifest_belongs_to_wit_root(&manifest_path, wit_root)?;

    let manifest = ReleasedManifest::load(&manifest_path)?;
    if !manifest.contains(&entry_name) {
        let diag = Diagnostic::error(
            codes::LOCKFILE_ERROR,
            format!(
                "cannot retire `{entry_name}`: it is not listed in {manifest_file}; release \
                 `{entry_name}` first"
            ),
        )
        .with_path(manifest_file.clone());
        bail!(ContractError::with_located_diagnostic(
            LocatedDiagnostic::from_diagnostic(diag).with_file(manifest_file)
        ));
    }

    // Validate retirement order BEFORE the idempotent already-retired return so a corrupt manifest
    // never silently exits 0.
    let versions = discover_version_directories(wit_root)?;
    let mut first_live: Option<String> = None;
    for (_, _, dir) in &versions {
        let name = dir
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or_default();
        if manifest.is_retired(name) || name == entry_name {
            if let Some(ref older_live) = first_live {
                let diag = Diagnostic::error(
                    codes::LOCKFILE_ERROR,
                    format!(
                        "cannot retire `{name}` while older version `{older_live}` is still live; \
                         only the oldest versions of a WIT root may be retired"
                    ),
                )
                .with_path(manifest_file.clone());
                bail!(ContractError::with_located_diagnostic(
                    LocatedDiagnostic::from_diagnostic(diag).with_file(manifest_file)
                ));
            }
        } else if first_live.is_none() {
            first_live = Some(name.to_string());
        }
    }
    if first_live.is_none() {
        let diag = Diagnostic::error(
            codes::LOCKFILE_ERROR,
            format!(
                "cannot retire `{entry_name}`: every version directory under {} would be retired; \
                 at least one live version is required",
                wit_root.display()
            ),
        )
        .with_path(manifest_file.clone());
        bail!(ContractError::with_located_diagnostic(
            LocatedDiagnostic::from_diagnostic(diag).with_file(manifest_file)
        ));
    }

    if manifest.is_retired(&entry_name) {
        if reporter.human() && !reporter.is_quiet() {
            reporter.write_stdout_line(&format!(
                "`{entry_name}` is already retired in {manifest_file}"
            ));
        }
        return Ok(ExitCode::SUCCESS);
    }

    let raw = fs::read_to_string(&manifest_path)
        .with_context(|| format!("failed to read {}", manifest_path.display()))?;
    let updated = workflow::retire_released_entry_text(&raw, &entry_name)?;
    workflow::write_atomic_manifest(&manifest_path, &updated)?;
    if reporter.human() && !reporter.is_quiet() {
        reporter.write_stdout_line(&format!(
            "retired `{entry_name}` in {}",
            manifest_path.display()
        ));
    }
    Ok(ExitCode::SUCCESS)
}

fn conversion_blockers(report: &DiffReport) -> impl Iterator<Item = (&'static str, &diff::Change)> {
    report
        .changes
        .iter()
        .filter_map(|change| match &change.kind {
            ChangeKind::Added { defaulted: false }
                if change.element == ChangeElement::RecordField =>
            {
                Some(("required-addition", change))
            }
            ChangeKind::Retyped { .. }
                if matches!(
                    change.element,
                    ChangeElement::RecordField | ChangeElement::VariantCase | ChangeElement::Type
                ) =>
            {
                Some(("retyped", change))
            }
            _ => None,
        })
}

fn diff_json(full: &diff::FullDiff) -> Value {
    let analysis = &full.analysis;
    let omissions = &full.conversion_omissions;
    let report = &analysis.report;
    let cross_major = report.old.version.major != report.new.version.major;
    let endpoint = |endpoint: &diff::Endpoint| {
        json!({
            "path": endpoint.path.display().to_string(),
            "package": endpoint.package,
            "version": endpoint.version.to_string(),
        })
    };
    let changes: Vec<Value> = report
        .changes
        .iter()
        .zip(full.decode_verdicts.iter())
        .map(|(change, verdict)| change_json(change, *verdict))
        .collect();
    let conversion_impacts: Vec<Value> = analysis
        .conversion_impacts
        .iter()
        .map(|impact| {
            json!({
                "type_path": &impact.type_path,
                "from_version": impact.from_version.to_string(),
                "to_version": impact.to_version.to_string(),
                "missing_member": &impact.missing_member,
                "missing_path": &impact.missing_path,
            })
        })
        .collect();
    let conversion_blockers: Vec<Value> = if cross_major {
        Vec::new()
    } else {
        conversion_blockers(report)
            .map(|(kind, change)| {
                json!({
                    "kind": kind,
                    "path": &change.path,
                })
            })
            .collect()
    };
    let conversion_omissions: Vec<Value> = omissions
        .iter()
        .map(|omission| {
            json!({
                "from_version": omission.from_version.to_string(),
                "to_version": omission.to_version.to_string(),
                "type_path": &omission.type_path,
                "reason": &omission.reason,
            })
        })
        .collect();
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
            "decode_breaking": full.decode_summary.breaking,
            "new_reads_old_breaking": full.decode_summary.new_reads_old_breaking,
            "old_reads_new_breaking": full.decode_summary.old_reads_new_breaking,
        },
        "conversion_impacts": conversion_impacts,
        "conversion_blockers": conversion_blockers,
        "conversion_omissions": conversion_omissions,
    })
}

fn change_json(change: &diff::Change, decode: diff::DecodeVerdict) -> Value {
    let mut value = json!({
        "element": change.element.as_str(),
        "path": &change.path,
        "number": change.number,
        "wire_breaking": change.wire_breaking,
        "source_breaking": change.source_breaking,
        "api_breaking": change.api_breaking,
        "decode_breaking": decode.is_breaking(),
        "new_reads_old": decode.new_reads_old,
        "old_reads_new": decode.old_reads_new,
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
        ChangeKind::StabilityChanged { from, to } => {
            ("stability-changed", json!({ "from": from, "to": to }))
        }
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
        ChangeKind::StabilityChanged { from, to } => {
            ("stability-changed", format!("{from} -> {to}"))
        }
    }
}

fn render_diff(
    report: &DiffReport,
    decode_verdicts: &[diff::DecodeVerdict],
    decode_summary: diff::DecodeSummary,
    gated_breaking: Option<usize>,
    quiet: bool,
) -> String {
    use std::fmt::Write as _;

    let mut out = String::new();
    if !quiet {
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
    }
    for (change, &verdict) in report.changes.iter().zip(decode_verdicts.iter()) {
        let (kind, detail) = change_label(change);
        let number = change
            .number
            .map_or_else(String::new, |number| format!(" number={number}"));
        let _ = writeln!(
            out,
            "{kind:<17} {:<18} {:<32}{number:<10}  wire:{} source:{} api:{} decode:{}  {detail}",
            change.element,
            change.path,
            status(change.wire_breaking),
            status(change.source_breaking),
            status(change.api_breaking),
            decode_status(verdict),
        );
    }
    if !quiet {
        let _ = writeln!(
            out,
            "summary: {} change(s), {} breaking; wire identity: {}, source: {}, generated API: {}, \
             decode: {} (new<-old: {}, old<-new: {})",
            report.summary.changes,
            report.summary.breaking,
            report.summary.wire_breaking,
            report.summary.source_breaking,
            report.summary.api_breaking,
            decode_summary.breaking,
            decode_summary.new_reads_old_breaking,
            decode_summary.old_reads_new_breaking,
        );
        let is_breaking = gated_breaking.map_or(report.summary.breaking > 0, |count| count > 0);
        if is_breaking {
            let _ = writeln!(out, "result: BREAKING");
        } else {
            let _ = writeln!(out, "result: COMPATIBLE");
        }
    }
    out
}

fn decode_status(verdict: diff::DecodeVerdict) -> &'static str {
    match (verdict.new_reads_old, verdict.old_reads_new) {
        (true, true) => "compatible",
        (false, true) => "BREAKING(new<-old)",
        (true, false) => "BREAKING(old<-new)",
        (false, false) => "BREAKING(both)",
    }
}

fn status(breaking: bool) -> &'static str {
    if breaking { "BREAKING" } else { "compatible" }
}

fn check_findings(
    findings: &[emit::output::GeneratedFileFinding],
    reporter: &mut Reporter,
) -> ExitCode {
    for finding in findings {
        reporter.push_located(finding.diagnostic());
    }
    if findings.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

/// The `v<major>.<minor>` a version directory name spells.
fn version_directory(path: &Path) -> Option<(u64, u64)> {
    let name = path.file_name()?.to_str()?;
    let (major, minor) = name.strip_prefix('v')?.split_once('.')?;
    let canonical = |digits: &str| {
        !digits.is_empty()
            && digits.bytes().all(|byte| byte.is_ascii_digit())
            && (digits == "0" || !digits.starts_with('0'))
    };
    if !canonical(major) || !canonical(minor) {
        return None;
    }
    Some((major.parse().ok()?, minor.parse().ok()?))
}

/// The highest sibling version directory of `dir` in the same major, if any.
fn sibling_predecessor(dir: &Path) -> Option<(u64, PathBuf)> {
    let (major, minor) = version_directory(dir)?;
    let parent = dir.parent()?;
    let mut best: Option<(u64, PathBuf)> = None;
    for entry in fs::read_dir(parent).ok()?.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let Some((sibling_major, sibling_minor)) = version_directory(&path) else {
            continue;
        };
        if sibling_major == major
            && sibling_minor < minor
            && best
                .as_ref()
                .is_none_or(|(best_minor, _)| sibling_minor > *best_minor)
        {
            best = Some((sibling_minor, path));
        }
    }
    best
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

#[cfg(test)]
mod reference_doc_tests {
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn reference_guide_documents_every_cli_command_option_and_config_key() {
        let doc_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/guide/reference.md");
        let doc = fs::read_to_string(&doc_path)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", doc_path.display()));
        let root = Cli::command();

        // Every global CLI flag on the root command must be documented in `docs/guide/reference.md`.
        for arg in root.get_arguments() {
            if let Some(long) = arg.get_long() {
                let needle = format!("`--{long}");
                assert!(
                    doc.contains(&needle),
                    "docs/guide/reference.md does not document global flag `--{long}`"
                );
            }
            if let Some(short) = arg.get_short() {
                let needle = format!("`-{short}`");
                assert!(
                    doc.contains(&needle),
                    "docs/guide/reference.md does not document global short flag `-{short}`"
                );
            }
        }

        // Every subcommand and every one of its non-global options must appear under its section.
        let global_longs: BTreeSet<String> = root
            .get_arguments()
            .filter_map(|a| a.get_long().map(str::to_string))
            .collect();
        for sub in root.get_subcommands() {
            let name = sub.get_name();
            if name == "help" {
                continue;
            }
            let heading = format!("### `witforge {name}`");
            let (_, after) = doc
                .split_once(&heading)
                .unwrap_or_else(|| panic!("docs/guide/reference.md missing section `{heading}`"));
            let section = after
                .split("\n### ")
                .next()
                .and_then(|s| s.split("\n## ").next())
                .unwrap_or(after);
            for arg in sub.get_arguments() {
                if let Some(long) = arg.get_long() {
                    if global_longs.contains(long) || long == "help" || long == "version" {
                        continue;
                    }
                    let needle = format!("`--{long}");
                    assert!(
                        section.contains(&needle),
                        "docs/guide/reference.md section `{heading}` does not document `--{long}`"
                    );
                }
            }
        }

        // Every `witforge.toml` key supported by `ConfigFile` / `ProjectConfig` / emitter structs.
        for key in [
            "`version`",
            "`[project.<name>]`",
            "`project.<name>.root`",
            "`project.<name>.released`",
            "`[project.<name>.rust]`",
            "`project.<name>.rust.out`",
            "`project.<name>.rust.serde`",
            "`project.<name>.rust.no-std`",
            "`[project.<name>.proto]`",
            "`project.<name>.proto.out`",
            "`[project.<name>.schema]`",
            "`project.<name>.schema.out`",
            "`project.<name>.schema.draft`",
            "`[project.<name>.glue]`",
            "`project.<name>.glue.out`",
            "`project.<name>.glue.plain-root`",
            "`project.<name>.glue.proto-root`",
            "`project.<name>.glue.no-std`",
        ] {
            assert!(
                doc.contains(key),
                "docs/guide/reference.md does not document witforge.toml key {key}"
            );
        }
    }
}

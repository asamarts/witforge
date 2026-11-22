//! The `witforge` command-line interface.
//!
//! ```text
//! witforge check [<path>] [--lock PATH] [--first | --against PATH] [--released PATH]
//!                [--config PATH | --no-config] [--project NAME]
//! witforge lock  <wit-dir> [--lock PATH] [--first | --against PATH] [--released PATH]
//! witforge rust  [<wit-root>] [--out DIR] [--config PATH | --no-config] [--project NAME]
//!                [--serde | --no-serde] [--check] [--diff] [--released PATH]
//! witforge proto [<wit-root>] [--out DIR] [--config PATH | --no-config] [--project NAME]
//!                [--check] [--diff] [--released PATH]
//! witforge schema [<wit-root>] [--out DIR] [--config PATH | --no-config] [--project NAME]
//!                [--check] [--diff] [--released PATH]
//! witforge glue  [<wit-root>] [--out DIR] [--plain-root PATH] [--proto-root PATH]
//!                [--config PATH | --no-config] [--project NAME] [--check] [--diff] [--released PATH]
//! witforge generate [--config PATH | --no-config] [--project NAME] [--check] [--diff]
//!                   [--serde | --no-serde] [--released PATH]
//! witforge new-version [<wit-root>] [--major] [--config PATH | --no-config] [--project NAME]
//! witforge release <wit-dir> [--released PATH] [--config PATH | --no-config] [--project NAME]
//! witforge diff  <old> <new> [--json] [--check[=VIEWS]]
//! witforge freeze [<wit-root>] [--config PATH | --no-config] [--project NAME] [--base REV]
//! witforge completions <bash|zsh|fish>
//! witforge explain <code-id>
//!
//! All commands accept `--format human|json|sarif` (`human` default) and `-q`/`--quiet`.
//! ```

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use anyhow::{Context, Result, bail};
use clap::{Args, CommandFactory, Parser, Subcommand};
use serde::Deserialize;

use serde_json::{Value, json};
use witforge::diagnostic::{
    Diagnostic, LocatedDiagnostic, Position, RelatedLocation, Severity, codes,
};
use witforge::diff::{self, ChangeKind, DiffReport};
use witforge::emit;
use witforge::lock::{LockMode, PackageName, WireLock};
mod completions;
mod diagnostics;
mod workflow;

use diagnostics::{Format, Reporter, diagnostic_path, located_note};

use witforge::{
    ContractError, DiagnosticError, ReleasedManifest, default_released_manifest,
    discover_version_directories, ensure_directory_version, ensure_lockfile_not_released,
    ensure_no_symlinks_in_dir, ensure_not_released, ensure_version_directory, error_diagnostics,
    error_located_diagnostics, is_contract_error, is_released, load, load_released_manifest,
    load_root_released_manifest, model, subset,
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

fn op_error_at(
    code: &'static str,
    message: impl Into<String>,
    file: String,
    position: Option<Position>,
) -> anyhow::Error {
    let mut diagnostic = Diagnostic::error(code, message).with_path(file.clone());
    if let Some(position) = position {
        diagnostic = diagnostic.with_position(position);
    }
    let located = LocatedDiagnostic::from_diagnostic(diagnostic).with_file(file);
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
    /// Generate Draft 7 JSON Schema definitions.
    Schema(SchemaOptions),
    /// Generate protobuf glue conversions and function helpers.
    Glue(GlueOptions),
    /// Run every generator configured in `witforge.toml`.
    Generate(GenerateOptions),
    /// Create the next minor or major version directory from the latest version.
    #[command(name = "new-version")]
    NewVersion(NewVersionOptions),
    /// Validate a version directory and append it to the `released` manifest.
    Release(ReleaseOptions),
    /// Compare two contract versions and classify breaking changes.
    Diff(DiffOptions),
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
    /// api; defaults to all views when given without `=...`).
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
struct FreezeOptions {
    /// WIT root containing version directories and `released` manifest.
    #[arg(value_name = "WIT_ROOT")]
    root: Option<PathBuf>,
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
    /// Released-version manifest to validate; overrides a project's `released` setting.
    #[arg(long, value_name = "PATH")]
    released: Option<PathBuf>,
    /// Configuration file; discovered upwards from the working directory when omitted.
    #[command(flatten)]
    config: ConfigArgs,
}

#[derive(Debug, Args)]
struct NewVersionOptions {
    /// WIT root containing version directories; omit to use the configured project root.
    #[arg(value_name = "WIT_ROOT")]
    root: Option<PathBuf>,
    /// Start the next major version (`v<major+1>.0`) with no copied wire.lock instead of the next minor.
    #[arg(long)]
    major: bool,
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
struct RustConfig {
    out: PathBuf,
    /// Add serde derives to generated records, enums, variants, and flags.
    serde: Option<bool>,
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

struct PathOverrides {
    root: Option<PathBuf>,
    out: Option<PathBuf>,
    released: Option<PathBuf>,
    plain_root: Option<String>,
    proto_root: Option<String>,
    serde: Option<bool>,
}

struct ResolvedPaths {
    root: PathBuf,
    out: PathBuf,
    released: Option<PathBuf>,
    plain_root: Option<String>,
    proto_root: Option<String>,
    serde: bool,
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
        Emitter::Schema => project.schema.map(|config| from_config(&config.out)),
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
    Ok(ResolvedPaths {
        root,
        out,
        released,
        plain_root,
        proto_root,
        serde,
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
        Command::NewVersion(options) => new_version_command(&options, &mut reporter),
        Command::Release(options) => release_command(&options, &mut reporter),
        Command::Diff(options) => diff_command(&options, &mut reporter),
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
        let link_meta = fs::symlink_metadata(dir)
            .with_context(|| format!("failed to read version directory {}", dir.display()))?;
        if link_meta.file_type().is_symlink() {
            // A WIT root may be a symlink, but a symlinked version directory or dangling symlink
            // is refused with WF9004.
            let is_wit_root = fs::metadata(dir)
                .ok()
                .filter(fs::Metadata::is_dir)
                .and_then(|_| workflow::classify_check_dir(dir).ok())
                == Some(workflow::CheckTargetKind::WitRoot);
            if !is_wit_root {
                ensure_version_directory(dir)?;
            }
        } else if !link_meta.is_dir() {
            ensure_version_directory(dir)?;
        }
        return match workflow::classify_check_dir(dir)? {
            workflow::CheckTargetKind::VersionDir => {
                let single = Options {
                    dir: dir.clone(),
                    lock: options.lock.clone(),
                    against: options.against.clone(),
                    first: options.first,
                    released: options.released.clone(),
                };
                check_single(&single, reporter)
            }
            workflow::CheckTargetKind::WitRoot => {
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
            workflow::CheckTargetKind::Both => Err(op_error(
                codes::CLI_USAGE,
                format!(
                    "`{}` contains both WIT source/lockfile and `v<major>.<minor>` version \
                     directories; expected either a single version directory or a WIT root",
                    dir.display()
                ),
                Some(dir.display().to_string()),
            )),
            workflow::CheckTargetKind::Neither => {
                // Still check for malformed version-like entries (`v00.1`, `v0.1.1`) or symlinks.
                ensure_version_directory(dir)?;
                let _ = discover_version_directories(dir)?;
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

/// Validate every version directory under `wit_root` in ascending semver order:
/// released directories in default mode, the lowest minor of each major as `--first`, and every
/// later minor `--against` its immediate same-major predecessor, plus cross-version package,
/// dependency-graph, and lockfile-continuity checks.
#[allow(clippy::too_many_lines)]
fn check_wit_root(
    wit_root: &Path,
    explicit_released: Option<&Path>,
    reporter: &mut Reporter,
) -> Result<ExitCode> {
    let manifest = load_root_released_manifest(wit_root, explicit_released)?;
    let versions = discover_version_directories(wit_root)?;
    let mut prev_by_major: BTreeMap<u64, PathBuf> = BTreeMap::new();
    let mut first_model: Option<model::Model> = None;
    let mut failed = false;
    for (major, _minor, dir) in versions {
        let dir_name = dir
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .unwrap_or_default();
        let released = manifest.contains(dir_name) || is_released(&dir, explicit_released)?;
        let prev_dir = prev_by_major.get(&major).cloned();
        let (first, against) = if released {
            (false, None)
        } else if let Some(ref pred) = prev_dir {
            let pred_lock = pred.join("wire.lock");
            if !pred_lock.is_file() {
                failed = true;
                reporter.error(
                    codes::LOCKFILE_ERROR,
                    format!(
                        "cannot check {} against predecessor {}: missing lockfile {}",
                        dir.display(),
                        pred.display(),
                        pred_lock.display()
                    ),
                    Some(pred_lock.display().to_string()),
                );
                prev_by_major.insert(major, dir);
                continue;
            }
            (false, Some(pred_lock))
        } else {
            (true, None)
        };
        let opts = Options {
            dir: dir.clone(),
            lock: None,
            against: against.clone(),
            first,
            released: explicit_released.map(Path::to_path_buf),
        };
        match check_single(&opts, reporter) {
            Ok(code) => {
                if code != ExitCode::SUCCESS {
                    failed = true;
                }
            }
            Err(err) if is_contract_error(&err) => {
                failed = true;
                report_error_to_reporter(&err, reporter);
            }
            Err(err) => return Err(err),
        }
        // Cross-version checks: package identity, dependency graph, and released continuity.
        if let Ok(contract) = load(&dir)
            && subset::check_diagnostics(&contract).is_empty()
            && let Ok(current_model) = model::build(&contract)
        {
            if let Some(ref baseline) = first_model {
                if baseline.package.namespace != current_model.package.namespace
                    || baseline.package.name != current_model.package.name
                {
                    failed = true;
                    reporter.error(
                        codes::PACKAGE_MISMATCH,
                        format!(
                            "{} declares package {}:{}, expected {}:{}",
                            dir.display(),
                            current_model.package.namespace,
                            current_model.package.name,
                            baseline.package.namespace,
                            baseline.package.name
                        ),
                        Some(dir.display().to_string()),
                    );
                } else if let Err(err) =
                    witforge::ensure_same_dependency_graph(baseline, &current_model)
                {
                    failed = true;
                    reporter.error(
                        codes::DEPENDENCY_GRAPH_MISMATCH,
                        format!("{err:#}"),
                        Some(dir.display().to_string()),
                    );
                }
            } else {
                first_model = Some(current_model);
            }
            if released
                && let Some(ref pred) = prev_dir
                && let Ok((cur_lock, _)) = read_required_lock(&dir.join("wire.lock"))
                && let Ok((pred_lock, _)) = read_required_lock(&pred.join("wire.lock"))
                && let Err(err) = cur_lock.check_append_only(&pred_lock)
            {
                failed = true;
                reporter.error(
                    codes::LOCKFILE_ERROR,
                    format!(
                        "{} does not continue {}: {err:#}",
                        dir.join("wire.lock").display(),
                        pred.join("wire.lock").display()
                    ),
                    Some(dir.join("wire.lock").display().to_string()),
                );
            }
        }
        prev_by_major.insert(major, dir);
    }
    Ok(if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    })
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

#[allow(clippy::too_many_lines)]
fn check_single(options: &Options, reporter: &mut Reporter) -> Result<ExitCode> {
    ensure_version_directory(&options.dir)?;
    ensure_not_wit_root(&options.dir, "check")?;
    // `check` may verify a released directory; the manifest only has to be valid.
    load_released_manifest(&options.dir, options.released.as_deref())?;
    let contract = load(&options.dir)?;
    let violations = subset::check_located_diagnostics(&contract);
    if !violations.is_empty() {
        // The model of a contract outside the subset is not built, so every locked member
        // would look removed; report only the violations, as `lock` does.
        for violation in violations {
            reporter.push_subset_located(violation);
        }
        return Ok(ExitCode::from(1));
    }
    let member_locs = contract.member_locations();
    let model = model::build(&contract)?;
    ensure_directory_version(&options.dir, &model)?;
    ensure_first_is_allowed(options, "check")?;
    ensure_baseline_is_allowed(options, "check")?;
    let members = model.members()?;
    let lock_path = options
        .lock
        .clone()
        .unwrap_or_else(|| options.dir.join("wire.lock"));
    let (lock, text) = read_lock(&lock_path, options)?;
    let lock_positions = text
        .as_deref()
        .map(WireLock::entry_positions)
        .unwrap_or_default();
    let (previous, against_positions) = read_previous(options.against.as_deref())?;
    if let Some(against) = &options.against {
        if let Some(baseline) = baseline_model(against)? {
            if let Err(err) = witforge::ensure_same_dependency_graph(&model, &baseline) {
                reporter.error(
                    codes::DEPENDENCY_GRAPH_MISMATCH,
                    format!("{err:#}"),
                    Some(against.display().to_string()),
                );
                return Ok(ExitCode::from(1));
            }
        } else {
            let against_str = against.display().to_string();
            let diag = Diagnostic::new(
                codes::LOCKFILE_BASELINE_NO_DEPS,
                Severity::Note,
                format!(
                    "cannot compare the pinned dependency graph: baseline `{against_str}` is \
                     not a version-directory wire.lock"
                ),
            )
            .with_path(against_str.clone());
            reporter.note_located(
                &format!(
                    "note: cannot compare the pinned dependency graph: baseline \
                     `{against_str}` is not the wire.lock of a version directory; run a \
                     generator over the WIT root"
                ),
                LocatedDiagnostic::from_diagnostic(diag).with_file(against_str),
            );
        }
    }
    let released = is_released(&options.dir, options.released.as_deref())?;
    let remedy = if released {
        Remedy::Released(options.dir.display().to_string())
    } else {
        Remedy::Lock(lock_command(options))
    };
    let package = package_name(&model);
    if !check_lockfile_packages(
        &package,
        &lock,
        &lock_path,
        &lock_positions,
        previous.as_ref(),
        options.against.as_deref(),
        &against_positions,
        reporter,
    ) {
        return Ok(ExitCode::from(1));
    }
    let mode = lock_mode(options, previous.as_ref());
    let case_collisions: BTreeSet<(String, String)> = lock
        .case_collisions(&members, mode)
        .unwrap_or_default()
        .into_iter()
        .map(|diagnostic| (diagnostic.path.unwrap_or_default(), diagnostic.message))
        .collect();
    let report = match lock.drift_report_with(&members, mode) {
        Ok(report) => report,
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
    // A missing or wrong order section is checked when the entry set is current. A reorder
    // with a current entry set still reaches this check in every mode. A non-canonical
    // rendering is already reported above; the semantic check adds nothing to it.
    if canonical
        && report.is_empty()
        && let Err(err) = lock.validate_order_section(&members, released)
    {
        reporter.error(
            "WF1100",
            format!("{err:#}; {}", remedy.advice(" to update the order section")),
            Some(lock_path.display().to_string()),
        );
        return Ok(ExitCode::from(1));
    }
    if canonical
        && report.is_empty()
        && let Err(err) = emit::proto::emit_major(&model, &lock)
    {
        if is_contract_error(&err) {
            if let Some(diags) = error_located_diagnostics(&err) {
                for located in diags {
                    let mut located = located.clone();
                    if located.file.is_none() {
                        located.file = Some(lock_path.display().to_string());
                    }
                    if located.diagnostic.path.is_none() {
                        located.diagnostic = located
                            .diagnostic
                            .with_path(lock_path.display().to_string());
                    }
                    reporter.push_located(located);
                }
            } else {
                reporter.error(
                    codes::IDENTIFIER_COLLISION,
                    format!("{err:#}"),
                    Some(lock_path.display().to_string()),
                );
            }
            return Ok(ExitCode::from(1));
        }
        return Err(err);
    }
    let lock_ctx = LockDiagnosticContext {
        member_locs: &member_locs,
        lock_file: &lock_path.display().to_string(),
        lock_positions: &lock_positions,
        against_file: options.against.as_deref().map(|p| p.display().to_string()),
        against_positions: &against_positions,
    };
    report_check_changes(
        &report.changes,
        &report.discrepancies,
        &case_collisions,
        &report.restored,
        options.against.is_some(),
        &remedy,
        &lock_ctx,
        reporter,
    );
    if report.is_empty() && canonical {
        Ok(ExitCode::SUCCESS)
    } else {
        Ok(ExitCode::from(1))
    }
}

#[allow(clippy::too_many_lines)]
fn lock(options: &Options, reporter: &mut Reporter) -> Result<ExitCode> {
    ensure_not_released(&options.dir, options.released.as_deref())?;
    ensure_not_wit_root(&options.dir, "lock")?;
    let contract = load(&options.dir)?;
    let violations = subset::check_located_diagnostics(&contract);
    let violation_count = violations.len();
    for violation in violations {
        reporter.push_subset_located(violation);
    }
    if violation_count > 0 {
        if reporter.human() {
            bail!(ContractError::new(format!(
                "{violation_count} subset violation(s); the lockfile was not updated"
            )));
        }
        return Ok(ExitCode::from(1));
    }
    let member_locs = contract.member_locations();
    let model = model::build(&contract)?;
    ensure_directory_version(&options.dir, &model)?;
    let members = model.members()?;
    let lock_path = options
        .lock
        .clone()
        .unwrap_or_else(|| options.dir.join("wire.lock"));
    ensure_lockfile_not_released(&lock_path, &options.dir, options.released.as_deref())?;
    ensure_first_is_allowed(options, "lock")?;
    ensure_baseline_is_allowed(options, "lock")?;
    let (mut lock, existing_text) = read_lock(&lock_path, options)?;
    let lock_positions = existing_text
        .as_deref()
        .map(WireLock::entry_positions)
        .unwrap_or_default();
    let (previous, against_positions) = read_previous(options.against.as_deref())?;
    let package = package_name(&model);
    if !check_lockfile_packages(
        &package,
        &lock,
        &lock_path,
        &lock_positions,
        previous.as_ref(),
        options.against.as_deref(),
        &against_positions,
        reporter,
    ) {
        return Ok(ExitCode::from(1));
    }
    let mode = lock_mode(options, previous.as_ref());
    let case_collisions: BTreeSet<(String, String)> = lock
        .case_collisions(&members, mode)
        .unwrap_or_default()
        .into_iter()
        .map(|diagnostic| (diagnostic.path.unwrap_or_default(), diagnostic.message))
        .collect();
    // Every refusal from reconciling is a lockfile violation, as `check` reports it.
    let report = match lock.reconcile_report_with(&members, mode) {
        Ok(report) => report,
        Err(err) => {
            reporter.error(
                codes::LOCKFILE_ERROR,
                format!("{err:#}"),
                Some(lock_path.display().to_string()),
            );
            return Ok(ExitCode::from(1));
        }
    };
    let lock_file = lock_path.display().to_string();
    let lock_ctx = LockDiagnosticContext {
        member_locs: &member_locs,
        lock_file: &lock_file,
        lock_positions: &lock_positions,
        against_file: options.against.as_deref().map(|p| p.display().to_string()),
        against_positions: &against_positions,
    };
    let changes = &report.changes;
    let mut refused = false;
    for discrepancy in &report.discrepancies {
        let code =
            if case_collisions.contains(&(discrepancy.key.clone(), discrepancy.message.clone())) {
                "WF1003"
            } else {
                "WF1100"
            };
        reporter.push_located(lock_ctx.locate_discrepancy(
            code,
            &discrepancy.key,
            discrepancy.message.clone(),
        ));
        refused = true;
    }
    if !changes.reintroduced.is_empty() {
        for (key, number) in &changes.reintroduced {
            reporter.push_located(lock_ctx.locate_member(
                "WF1103",
                key,
                reintroduced(key, *number, options.against.is_some()),
                false,
                Some("reserved entry in wire.lock"),
            ));
            refused = true;
        }
    }
    if refused {
        if reporter.human() {
            bail!(ContractError::new(format!(
                "{} lockfile discrepancy(s); the lockfile was not updated",
                report.discrepancies.len() + changes.reintroduced.len()
            )));
        }
        return Ok(ExitCode::from(1));
    }
    lock.write_atomic(&lock_path)?;
    let written_positions = fs::read_to_string(&lock_path)
        .ok()
        .map(|text| WireLock::entry_positions(&text))
        .unwrap_or_default();
    report_lock_notes(
        &report,
        &member_locs,
        &lock_file,
        &written_positions,
        reporter,
    );
    Ok(ExitCode::SUCCESS)
}

#[allow(clippy::too_many_arguments)]
fn check_lockfile_packages(
    package: &PackageName,
    lock: &WireLock,
    lock_path: &Path,
    lock_positions: &BTreeMap<String, Position>,
    previous: Option<&WireLock>,
    against_path: Option<&Path>,
    against_positions: &BTreeMap<String, Position>,
    reporter: &mut Reporter,
) -> bool {
    if let Err(err) = lock.check_package(package) {
        let file = lock_path.display().to_string();
        let pos = lock
            .entries()
            .find(|(key, _)| key.package() != package)
            .and_then(|(key, _)| lock_positions.get(&key.key()).copied());
        let mut diag = Diagnostic::error(codes::LOCKFILE_WRONG_PACKAGE, format!("{err:#}"))
            .with_path(file.clone());
        if let Some(pos) = pos {
            diag = diag.with_position(pos);
        }
        reporter.push_located(LocatedDiagnostic::from_diagnostic(diag).with_file(file));
        return false;
    }
    if let (Some(previous), Some(against_path)) = (previous, against_path)
        && let Err(err) = previous.check_package(package)
    {
        let file = against_path.display().to_string();
        let pos = previous
            .entries()
            .find(|(key, _)| key.package() != package)
            .and_then(|(key, _)| against_positions.get(&key.key()).copied());
        let mut diag = Diagnostic::error(codes::LOCKFILE_WRONG_PACKAGE, format!("{err:#}"))
            .with_path(file.clone());
        if let Some(pos) = pos {
            diag = diag.with_position(pos);
        }
        reporter.push_located(LocatedDiagnostic::from_diagnostic(diag).with_file(file));
        return false;
    }
    true
}

fn report_lock_notes(
    report: &witforge::lock::Reconciliation,
    member_locs: &BTreeMap<String, (String, Position)>,
    lock_file: &str,
    lock_positions: &BTreeMap<String, Position>,
    reporter: &mut Reporter,
) {
    let changes = &report.changes;
    let emit =
        |reporter: &mut Reporter, code: &'static str, human: String, message: String, key: &str| {
            let lock_pos = lock_positions.get(key).copied();
            let wit_loc = member_locs.get(key);
            let located = if wit_loc.is_some() {
                located_note(
                    code,
                    message,
                    key.to_string(),
                    wit_loc,
                    Some((lock_file.to_string(), lock_pos, "lockfile entry")),
                )
            } else {
                let fallback = lock_pos.map(|pos| (lock_file.to_string(), pos));
                let mut item =
                    located_note(code, message, key.to_string(), fallback.as_ref(), None);
                if item.file.is_none() {
                    item = item.with_file(lock_file.to_string());
                }
                item
            };
            reporter.note_located(&human, located);
        };
    for (key, number) in &report.restored {
        emit(
            reporter,
            "WF1205",
            format!("restored {key} = {number}"),
            format!("restored {key} = {number}"),
            key,
        );
    }
    for (key, number) in &changes.dropped {
        emit(
            reporter,
            "WF1203",
            format!("dropped  {key} = {number}"),
            format!("dropped {key} = {number}"),
            key,
        );
    }
    for (key, _kind, _ty) in &changes.updated {
        emit(
            reporter,
            "WF1204",
            format!("updated  {key}"),
            format!("updated {key}"),
            key,
        );
    }
    for (key, number) in &changes.reserved {
        emit(
            reporter,
            "WF1202",
            format!("reserved {key} = {number}"),
            format!("reserved {key} = {number}"),
            key,
        );
    }
    for (key, number) in &changes.added {
        emit(
            reporter,
            "WF1201",
            format!("added    {key} = {number}"),
            format!("added {key} = {number}"),
            key,
        );
    }
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
        let messages = files.check_with_command(out, regen_command)?;
        if show_diff && reporter.human() && !messages.is_empty() {
            let diff_text = files.unified_diff(out)?;
            if !diff_text.is_empty() {
                reporter.write_stdout_line(diff_text.trim_end_matches('\n'));
            }
        }
        return Ok(check_messages(&messages, reporter));
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
        },
    )?;
    guard_output_root(&paths)?;
    let files = emit::rust::plan_with_serde(&paths.root, paths.serde)?;
    let regen = regenerate_command(
        "rust",
        &options.config,
        options.root.as_deref(),
        options.out.as_deref(),
        options.released.as_deref(),
        None,
        None,
        serde_override,
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
        },
    )?;
    guard_output_root(&paths)?;
    let files = emit::proto::plan(&paths.root)?;
    let regen = regenerate_command(
        "proto",
        &options.config,
        options.root.as_deref(),
        options.out.as_deref(),
        options.released.as_deref(),
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
        },
    )?;
    guard_output_root(&paths)?;
    let files = emit::json_schema::plan(&paths.root)?;
    let regen = regenerate_command(
        "schema",
        &options.config,
        options.root.as_deref(),
        options.out.as_deref(),
        options.released.as_deref(),
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

fn glue(options: &GlueOptions, reporter: &mut Reporter) -> Result<ExitCode> {
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
    // stay frozen. As for the other generators, the manifest only has to be valid.
    guard_output_root(&paths)?;
    let files = emit::glue::plan(&paths.root, plain_root, proto_root)?;
    let regen = regenerate_command(
        "glue",
        &options.config,
        options.root.as_deref(),
        options.out.as_deref(),
        options.released.as_deref(),
        options.plain_root.as_deref(),
        options.proto_root.as_deref(),
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
struct CheckViews {
    wire: bool,
    source: bool,
    api: bool,
}

impl CheckViews {
    fn all(self) -> bool {
        self.wire && self.source && self.api
    }

    fn matches(self, change: &diff::Change) -> bool {
        (self.wire && change.wire_breaking)
            || (self.source && change.source_breaking)
            || (self.api && change.api_breaking)
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
             `source`, `api`",
            None,
        ));
    }
    let mut views = CheckViews {
        wire: false,
        source: false,
        api: false,
    };
    for part in spec.split(',') {
        match part {
            "wire" => views.wire = true,
            "source" => views.source = true,
            "api" => views.api = true,
            "" => {
                return Err(op_error(
                    codes::CLI_USAGE,
                    format!(
                        "invalid `--check={spec}`: empty view name; expected `wire`, `source`, \
                         or `api`"
                    ),
                    None,
                ));
            }
            other => {
                return Err(op_error(
                    codes::CLI_USAGE,
                    format!(
                        "unknown `--check` view `{other}`; expected `wire`, `source`, or `api`"
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
    let (analysis, locations) = diff::compare_with_locations(&options.old, &options.new)?;
    let report = &analysis.report;
    if options.json {
        let rendered = serde_json::to_string_pretty(&diff_json(&analysis))
            .context("failed to render the diff as JSON")?;
        reporter.write_stdout_line(&rendered);
    } else if reporter.human() {
        let rendered = render_diff(report, reporter.is_quiet());
        if !rendered.is_empty() {
            reporter.write_stdout_line(rendered.trim_end_matches('\n'));
        }
    } else {
        for change in &report.changes {
            let (code, severity) = if change.is_breaking() {
                (
                    "WF3001",
                    if check_views.is_some_and(|views| views.matches(change)) {
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
        for (kind, blocker) in conversion_blockers(report) {
            let (human, message) = match kind {
                "required-addition" => (
                    format!(
                        "note: conversion generation refuses `{}`: a required record field was \
                         added without a default; use option<T>, list<T>, or a new member",
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
    if let Some(views) = check_views {
        let gated_breaking = report
            .changes
            .iter()
            .filter(|change| views.matches(change))
            .count();
        if gated_breaking > 0 {
            if reporter.human() || reporter.is_suppressed() {
                if views.all() {
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

fn freeze_command(options: &FreezeOptions, reporter: &mut Reporter) -> Result<ExitCode> {
    let roots = resolve_freeze_roots(options)?;
    let freeze_opts = witforge::freeze::FreezeOptions {
        base: options.base.clone(),
    };
    let mut failed = false;
    for root in roots {
        let report = witforge::freeze::check_root(&root, &freeze_opts)?;
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

fn resolve_freeze_roots(options: &FreezeOptions) -> Result<Vec<PathBuf>> {
    if let Some(root) = &options.root {
        if options.config.config.is_some() || options.config.project.is_some() {
            return Err(op_error(
                codes::CLI_USAGE,
                "`--config` and `--project` cannot be combined with a positional WIT_ROOT on `freeze`",
                None,
            ));
        }
        return Ok(vec![root.clone()]);
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
        return Ok(vec![resolve_config_path(&base, &project.root)]);
    }
    Ok(config
        .project
        .values()
        .map(|project| resolve_config_path(&base, &project.root))
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

    let serde_override = if options.serde {
        Some(true)
    } else if options.no_serde {
        Some(false)
    } else {
        None
    };

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

fn new_version_command(options: &NewVersionOptions, reporter: &mut Reporter) -> Result<ExitCode> {
    let wit_root = if let Some(root) = &options.root {
        if options.config.config.is_some() || options.config.project.is_some() {
            return Err(op_error(
                codes::CLI_USAGE,
                "`--config` and `--project` cannot be combined with a positional WIT_ROOT on `new-version`",
                None,
            ));
        }
        root.clone()
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
        resolve_config_path(&base, &project.root)
    };

    let (latest_dir, target_dir, copied_lock) =
        workflow::create_new_version(&wit_root, options.major)?;
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

    let manifest_path = if let Some(explicit) = &options.released {
        explicit.clone()
    } else if options.config.config.is_some() || options.config.project.is_some() {
        let Some(config_path) = resolve_config_path_opt(&options.config, true)? else {
            return Err(op_error(
                codes::CLI_USAGE,
                "`--project` requires `--config`",
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
        project.released.as_deref().map_or_else(
            || default_released_manifest(&options.dir),
            |p| resolve_config_path(&base, p),
        )
    } else {
        default_released_manifest(&options.dir)
    };

    if fs::symlink_metadata(&manifest_path).is_ok_and(|meta| meta.file_type().is_symlink()) {
        return Err(op_error(
            codes::GUARD_REFUSED,
            format!(
                "released manifest `{}` is a symlink; commit a regular file",
                manifest_path.display()
            ),
            Some(manifest_path.display().to_string()),
        ));
    }

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
            bail!(ContractError::with_diagnostic(
                Diagnostic::error(
                    codes::LOCKFILE_ERROR,
                    format!(
                        "cannot release `{entry_name}`: predecessor `{pred_name}` is not listed \
                         in {}; release `{pred_name}` first",
                        manifest_path.display()
                    ),
                )
                .with_path(options.dir.display().to_string())
            ));
        }
    }

    // Released directories must hold a real wire.lock and contain no symlinks.
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
    ensure_no_symlinks_in_dir(&options.dir, &entry_name)?;

    // Validate the version directory before recording the release.
    let check_opts = Options {
        dir: options.dir.clone(),
        lock: None,
        against: if already_listed {
            None
        } else {
            predecessor.as_ref().map(|(_, p)| p.join("wire.lock"))
        },
        first: !already_listed && predecessor.is_none(),
        released: manifest_path.exists().then(|| manifest_path.clone()),
    };
    let status = check_single(&check_opts, reporter)?;
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

fn conversion_blockers(report: &DiffReport) -> impl Iterator<Item = (&'static str, &diff::Change)> {
    report
        .changes
        .iter()
        .filter_map(|change| match &change.kind {
            ChangeKind::Added { defaulted: false } if change.element == "record field" => {
                Some(("required-addition", change))
            }
            ChangeKind::Retyped { .. }
                if matches!(
                    change.element.as_str(),
                    "record field" | "variant case" | "type"
                ) =>
            {
                Some(("retyped", change))
            }
            _ => None,
        })
}

fn diff_json(analysis: &diff::DiffAnalysis) -> Value {
    let report = &analysis.report;
    let endpoint = |endpoint: &diff::Endpoint| {
        json!({
            "path": endpoint.path.display().to_string(),
            "package": endpoint.package,
            "version": endpoint.version.to_string(),
        })
    };
    let changes: Vec<Value> = report.changes.iter().map(change_json).collect();
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
    let conversion_blockers: Vec<Value> = conversion_blockers(report)
        .map(|(kind, change)| {
            json!({
                "kind": kind,
                "path": &change.path,
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
        },
        "conversion_impacts": conversion_impacts,
        "conversion_blockers": conversion_blockers,
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

fn render_diff(report: &DiffReport, quiet: bool) -> String {
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
    if !quiet {
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
    }
    out
}

fn status(breaking: bool) -> &'static str {
    if breaking { "BREAKING" } else { "compatible" }
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

/// Load the WIT model beside a version-directory baseline lockfile.
///
/// A baseline copied outside a version directory has no WIT to compare and returns `None`; the
/// caller reports that the pinned dependency graph cannot be checked.
fn baseline_model(path: &Path) -> Result<Option<model::Model>> {
    let Some(parent) = path.parent() else {
        return Ok(None);
    };
    if path.file_name().and_then(|name| name.to_str()) != Some("wire.lock")
        || version_directory(parent).is_none()
    {
        return Ok(None);
    }
    let contract = load(parent)
        .with_context(|| format!("failed to load the baseline version {}", parent.display()))?;
    let violations = subset::check_located_diagnostics(&contract);
    if !violations.is_empty() {
        let messages: Vec<String> = violations
            .iter()
            .map(|located| match &located.diagnostic.path {
                Some(path) => format!("{path}: {}", located.diagnostic.message),
                None => located.diagnostic.message.clone(),
            })
            .collect();
        bail!(ContractError::with_located_diagnostics(
            format!("{}: {}", parent.display(), messages.join("; ")),
            violations,
        ));
    }
    Ok(Some(model::build(&contract)?))
}

fn same_parent(left: Option<&Path>, right: Option<&Path>) -> bool {
    let (Some(left), Some(right)) = (left, right) else {
        return false;
    };
    let left = fs::canonicalize(left).unwrap_or_else(|_| left.to_path_buf());
    let right = fs::canonicalize(right).unwrap_or_else(|_| right.to_path_buf());
    left == right
}

/// The command that continues `options`' directory against `baseline`.
fn against_command(options: &Options, verb: &str, baseline: &Path) -> String {
    let mut command = format!("witforge {verb} {}", shell_quote(&options.dir));
    if let Some(lock) = &options.lock {
        let _ = write!(command, " --lock {}", shell_quote(lock));
    }
    let _ = write!(command, " --against {}", shell_quote(baseline));
    if let Some(released) = &options.released {
        let _ = write!(command, " --released {}", shell_quote(released));
    }
    command
}

/// Refuse `--first` when the root already holds an earlier minor of the same major.
fn ensure_first_is_allowed(options: &Options, verb: &str) -> Result<()> {
    if !options.first {
        return Ok(());
    }
    let current = fs::canonicalize(&options.dir).unwrap_or_else(|_| options.dir.clone());
    let Some((major, _minor)) = version_directory(&current) else {
        return Ok(());
    };
    let Some((predecessor_minor, predecessor)) = sibling_predecessor(&current) else {
        return Ok(());
    };
    let baseline = predecessor.join("wire.lock");
    bail!(ContractError::with_diagnostic(
        Diagnostic::error(
            codes::LOCKFILE_ERROR,
            format!(
                "`--first` is refused for `{}`: major {major} already has {} at minor \
                 {predecessor_minor}, so a new minor continues that lockfile. Run `{}` instead",
                options.dir.display(),
                predecessor.display(),
                against_command(options, verb, &baseline)
            ),
        )
        .with_path(options.dir.display().to_string())
    ));
}

/// Refuse a baseline from a sibling version directory of another major or one that skips the
/// immediate predecessor.
fn ensure_baseline_is_allowed(options: &Options, verb: &str) -> Result<()> {
    let Some(against) = &options.against else {
        return Ok(());
    };
    if against.file_name().and_then(|name| name.to_str()) != Some("wire.lock") || !against.is_file()
    {
        // A baseline outside a version directory is accepted as given.
        return Ok(());
    }
    let current = fs::canonicalize(&options.dir).unwrap_or_else(|_| options.dir.clone());
    let Some((current_major, current_minor)) = version_directory(&current) else {
        return Ok(());
    };
    let Some(parent) = against.parent() else {
        return Ok(());
    };
    let Some((baseline_major, baseline_minor)) = version_directory(parent) else {
        return Ok(());
    };
    if !same_parent(current.parent(), parent.parent()) {
        return Ok(());
    }
    if baseline_major != current_major {
        bail!(ContractError::with_diagnostic(
            Diagnostic::error(
                codes::LOCKFILE_ERROR,
                format!(
                    "`--against {}` is the lockfile of major {baseline_major}, but `{}` is major \
                     {current_major}; a new major starts a fresh lockfile with `--first`. Run \
                     `witforge {verb} {} --first` instead",
                    against.display(),
                    options.dir.display(),
                    shell_quote(&options.dir)
                ),
            )
            .with_path(against.display().to_string())
        ));
    }
    if baseline_minor < current_minor
        && let Some((predecessor_minor, predecessor)) = sibling_predecessor(&current)
        && baseline_minor != predecessor_minor
    {
        bail!(ContractError::with_diagnostic(
            Diagnostic::error(
                codes::LOCKFILE_ERROR,
                format!(
                    "`--against {}` skips the immediate predecessor `{}`; run `{}` instead",
                    against.display(),
                    predecessor.display(),
                    against_command(options, verb, &predecessor.join("wire.lock"))
                ),
            )
            .with_path(against.display().to_string())
        ));
    }
    Ok(())
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

fn read_previous(path: Option<&Path>) -> Result<(Option<WireLock>, BTreeMap<String, Position>)> {
    match path {
        Some(path) => {
            let (lock, positions) = read_required_lock(path)?;
            Ok((Some(lock), positions))
        }
        None => Ok((None, BTreeMap::new())),
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
            let lock = WireLock::parse(&text).map_err(|err| {
                let position = error_diagnostics(&err)
                    .and_then(|diags| diags.first())
                    .and_then(|diag| diag.position);
                op_error_at(
                    codes::PARSE_ERROR,
                    format!("in {}: {err:#}", path.display()),
                    path.display().to_string(),
                    position,
                )
            })?;
            Ok((lock, Some(text)))
        }
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
            if options.first {
                return Ok((WireLock::default(), None));
            }
            if let Some(previous) = &options.against {
                return Err(op_error(
                    codes::IO_ERROR,
                    format!(
                        "lockfile {} does not exist; a new minor starts as a copy of the previous \
                         minor's lockfile: copy {} to {} first, then rerun `{}`",
                        path.display(),
                        previous.display(),
                        path.display(),
                        lock_command(options),
                    ),
                    Some(path.display().to_string()),
                ));
            }
            let current = fs::canonicalize(&options.dir).unwrap_or_else(|_| options.dir.clone());
            if let Some((major, _)) = version_directory(&current)
                && let Some((_, predecessor)) = sibling_predecessor(&current)
            {
                let prev_lock = predecessor.join("wire.lock");
                if prev_lock.is_file() {
                    return Err(op_error(
                        codes::IO_ERROR,
                        format!(
                            "lockfile {path} does not exist; major {major} already has \
                             {prev_dir}, so copy {prev_lock} to {path} and run `{against}` (or \
                             for a new major, create a fresh lockfile with `--first`)",
                            path = path.display(),
                            prev_dir = predecessor.display(),
                            prev_lock = prev_lock.display(),
                            against = against_command(options, "lock", &prev_lock),
                        ),
                        Some(path.display().to_string()),
                    ));
                }
            }
            Err(op_error(
                codes::IO_ERROR,
                format!(
                    "lockfile {path} does not exist; for the first version of a package or of a \
                     new major, create it with `{first}`; for a new minor, copy the previous \
                     minor's wire.lock to {path} and run witforge lock on that directory with \
                     --against pointing at the previous minor's wire.lock",
                    path = path.display(),
                    first = lock_command_with(options, Some("--first")),
                ),
                Some(path.display().to_string()),
            ))
        }
        Err(err) => Err(err).with_context(|| format!("failed to read {}", path.display())),
    }
}

/// Refuse a WIT root passed to a single-version command (`check` or `lock`) with a clear list of
/// the version directories found inside it.
fn ensure_not_wit_root(dir: &Path, verb: &str) -> Result<()> {
    let mut has_wit_files = false;
    let mut found: Vec<(u64, u64, String, PathBuf)> = Vec::new();
    let Ok(entries) = fs::read_dir(dir) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let entry_path = entry.path();
        if entry_path.is_file()
            && entry_path
                .extension()
                .and_then(std::ffi::OsStr::to_str)
                .is_some_and(|ext| ext.eq_ignore_ascii_case("wit"))
        {
            has_wit_files = true;
            break;
        }
        if entry_path.is_dir()
            && let Some((major, minor)) = version_directory(&entry_path)
            && let Some(name) = entry_path.file_name().and_then(std::ffi::OsStr::to_str)
        {
            found.push((major, minor, name.to_string(), entry_path));
        }
    }
    if has_wit_files || found.is_empty() {
        return Ok(());
    }
    found.sort_by_key(|(major, minor, _, _)| (*major, *minor));
    let names: Vec<&str> = found.iter().map(|(_, _, name, _)| name.as_str()).collect();
    let example = &found.last().expect("non-empty").3;
    Err(op_error(
        codes::CLI_USAGE,
        format!(
            "`{}` is a WIT root containing version directories ({}), not a version directory; \
             `{verb}` expects a single version directory (for example `witforge {verb} {}`)",
            dir.display(),
            names.join(", "),
            shell_quote(example),
        ),
        Some(dir.display().to_string()),
    ))
}

fn read_required_lock(path: &Path) -> Result<(WireLock, BTreeMap<String, Position>)> {
    let text =
        fs::read_to_string(path).with_context(|| format!("failed to read {}", path.display()))?;
    let lock = WireLock::parse(&text).map_err(|err| {
        let position = error_diagnostics(&err)
            .and_then(|diags| diags.first())
            .and_then(|diag| diag.position);
        op_error_at(
            codes::PARSE_ERROR,
            format!("in {}: {err:#}", path.display()),
            path.display().to_string(),
            position,
        )
    })?;
    let positions = WireLock::entry_positions(&text);
    Ok((lock, positions))
}

struct LockDiagnosticContext<'a> {
    member_locs: &'a BTreeMap<String, (String, Position)>,
    lock_file: &'a str,
    lock_positions: &'a BTreeMap<String, Position>,
    against_file: Option<String>,
    against_positions: &'a BTreeMap<String, Position>,
}

impl LockDiagnosticContext<'_> {
    fn lock_entry(&self, key: &str) -> (String, Option<Position>) {
        if let Some(pos) = self.lock_positions.get(key).copied() {
            return (self.lock_file.to_string(), Some(pos));
        }
        if let Some(ref against) = self.against_file
            && let Some(pos) = self.against_positions.get(key).copied()
        {
            return (against.clone(), Some(pos));
        }
        (self.lock_file.to_string(), None)
    }

    fn locate_member(
        &self,
        code: &'static str,
        key: &str,
        message: String,
        prefer_lock: bool,
        related_msg: Option<&str>,
    ) -> LocatedDiagnostic {
        let mut diag = Diagnostic::error(code, message).with_path(key.to_string());
        let wit_loc = self.member_locs.get(key);
        let (lock_file, lock_pos) = self.lock_entry(key);
        let mut located = if prefer_lock {
            if let Some(pos) = lock_pos {
                diag = diag.with_position(pos);
            }
            LocatedDiagnostic::from_diagnostic(diag)
                .with_file(lock_file)
                .with_logical(key.to_string())
        } else if let Some((file, pos)) = wit_loc {
            diag = diag.with_position(*pos);
            let mut item = LocatedDiagnostic::from_diagnostic(diag)
                .with_file(file.clone())
                .with_logical(key.to_string());
            if let Some(msg) = related_msg {
                item = item.with_related(RelatedLocation {
                    file: lock_file,
                    position: lock_pos,
                    message: msg.to_string(),
                });
            }
            item
        } else {
            if let Some(pos) = lock_pos {
                diag = diag.with_position(pos);
            }
            LocatedDiagnostic::from_diagnostic(diag)
                .with_file(lock_file)
                .with_logical(key.to_string())
        };
        if prefer_lock && let (Some((file, pos)), Some(msg)) = (wit_loc, related_msg) {
            located = located.with_related(RelatedLocation {
                file: file.clone(),
                position: Some(*pos),
                message: msg.to_string(),
            });
        }
        located
    }

    fn locate_discrepancy(
        &self,
        code: &'static str,
        key: &str,
        message: String,
    ) -> LocatedDiagnostic {
        // For a case collision against a reserved entry, find the conflicting entry case-insensitively.
        let mut diag = Diagnostic::error(code, message).with_path(key.to_string());
        let wit_loc = self.member_locs.get(key);
        let (lock_file, lock_pos) = self
            .lock_positions
            .get(key)
            .copied()
            .map(|pos| (self.lock_file.to_string(), Some(pos)))
            .or_else(|| {
                self.lock_positions
                    .iter()
                    .find(|(k, _)| k.eq_ignore_ascii_case(key))
                    .map(|(_, &pos)| (self.lock_file.to_string(), Some(pos)))
            })
            .or_else(|| {
                self.against_file.as_ref().and_then(|file| {
                    self.against_positions
                        .get(key)
                        .copied()
                        .or_else(|| {
                            self.against_positions
                                .iter()
                                .find(|(k, _)| k.eq_ignore_ascii_case(key))
                                .map(|(_, &pos)| pos)
                        })
                        .map(|pos| (file.clone(), Some(pos)))
                })
            })
            .unwrap_or_else(|| (self.lock_file.to_string(), None));
        if let Some((file, pos)) = wit_loc {
            diag = diag.with_position(*pos);
            LocatedDiagnostic::from_diagnostic(diag)
                .with_file(file.clone())
                .with_logical(key.to_string())
                .with_related(RelatedLocation {
                    file: lock_file,
                    position: lock_pos,
                    message: "conflicting entry in wire.lock".to_string(),
                })
        } else {
            if let Some(pos) = lock_pos {
                diag = diag.with_position(pos);
            }
            LocatedDiagnostic::from_diagnostic(diag)
                .with_file(lock_file)
                .with_logical(key.to_string())
        }
    }
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

#[allow(clippy::too_many_arguments)]
fn report_check_changes(
    changes: &witforge::lock::Changes,
    discrepancies: &[witforge::lock::Discrepancy],
    case_collisions: &BTreeSet<(String, String)>,
    restored: &[(String, u32)],
    against: bool,
    remedy: &Remedy,
    ctx: &LockDiagnosticContext<'_>,
    reporter: &mut Reporter,
) {
    for discrepancy in discrepancies {
        if case_collisions.contains(&(discrepancy.key.clone(), discrepancy.message.clone())) {
            reporter.push_located(ctx.locate_discrepancy(
                "WF1003",
                &discrepancy.key,
                discrepancy.message.clone(),
            ));
        } else {
            reporter.push_located(ctx.locate_discrepancy(
                "WF1100",
                &discrepancy.key,
                format!("{}; {}", discrepancy.message, remedy.advice("")),
            ));
        }
    }
    for (key, _) in restored {
        reporter.push_located(ctx.locate_member(
            "WF1108",
            key,
            format!(
                "draft removal of {key} can be restored; {}",
                remedy.advice("")
            ),
            false,
            Some("reserved entry in wire.lock"),
        ));
    }
    for (key, _) in &changes.added {
        reporter.push_located(ctx.locate_member(
            "WF1101",
            key,
            format!("{key} has no wire number; {}", remedy.advice("")),
            false,
            Some("wire.lock has no entry for this member"),
        ));
    }
    for (key, number) in &changes.reserved {
        reporter.push_located(ctx.locate_member(
            "WF1102",
            key,
            format!(
                "{key} = {number} is no longer in the contract; {}",
                remedy.advice(" to reserve it")
            ),
            true,
            None,
        ));
    }
    for (key, number) in &changes.reintroduced {
        reporter.push_located(ctx.locate_member(
            "WF1103",
            key,
            reintroduced(key, *number, against),
            false,
            Some("reserved entry in wire.lock"),
        ));
    }
    for (key, number) in &changes.dropped {
        reporter.push_located(ctx.locate_member(
            "WF1104",
            key,
            format!(
                "stale draft {key} = {number}; {}",
                remedy.advice(" to drop it")
            ),
            true,
            None,
        ));
    }
    for (key, _kind, _ty) in &changes.updated {
        reporter.push_located(ctx.locate_member(
            "WF1105",
            key,
            format!(
                "draft {key} changed kind or type; {}",
                remedy.advice(" to update it")
            ),
            false,
            Some("existing entry in wire.lock"),
        ));
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

fn reintroduced(key: &str, number: u32, against: bool) -> String {
    if against {
        format!(
            "{key} was removed and its name and number {number} stay reserved; give the member a new name"
        )
    } else {
        format!(
            "{key} was removed and its name and number {number} stay reserved; if the removal was a draft, use `--against` with the released baseline; otherwise give the member a new name"
        )
    }
}

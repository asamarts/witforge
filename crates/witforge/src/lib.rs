#![deny(missing_docs)]

//! witforge: a WIT-first contract compiler.
//!
//! A WIT package is the single source of truth for a contract. witforge checks that the package
//! stays inside a restricted, pure subset of WIT that maps cleanly onto every target, keeps a
//! committed, append-only lockfile of wire numbers so the protobuf projection stays stable across
//! versions, and emits plain Rust types and traits, version-to-version adapters, a `.proto`
//! schema, Draft 7 JSON Schema, and the glue that converts between the plain and protobuf types.
//!
//! - [`load`] parses a version directory's WIT package, and [`subset`] checks it against the
//!   supported subset.
//! - [`model`] is the owned contract model the lockfile and the emitters consume.
//! - [`lock`] reads, reconciles, and writes the `wire.lock` lockfile, and
//!   [`ReleasedManifest`] reads the `released` manifest of frozen versions.
//! - [`emit`] generates the [Rust](emit::rust) output (including version-to-version
//!   `convert.rs`), [protobuf](emit::proto), [JSON Schema](emit::json_schema), and
//!   [glue](emit::glue) output for a WIT root and checks committed output for drift.
//! - [`diagnostic`] defines the stable machine-readable diagnostics shared by the CLI commands.
//! - [`ContractError`] marks contract, lockfile, and output violations, as distinct from usage,
//!   I/O, and parse errors.
//!
//! The `witforge` command-line interface (the `witforge-cli` crate) wraps these as the `check`,
//! `lock`, `diff`, `rust`, `proto`, `schema`, and `glue` commands. The normative specifications
//! are in [`docs/design`](https://github.com/asamarts/witforge/tree/main/docs/design).

pub mod diagnostic;
pub mod diff;
pub mod emit;
pub mod lock;
pub mod model;
pub mod subset;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::path::{Component, Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use wit_parser::{
    AstItem, InterfaceId, PackageId, PackageName, Resolve, SourceMap, UnresolvedPackage,
    UnresolvedPackageGroup, WorldId, WorldKey,
};

/// A contract, lockfile, or generated-output violation, as opposed to a usage, I/O, parse, or
/// internal error.
///
/// The generators ([`emit::rust`], [`emit::proto`], [`emit::json_schema`], and [`emit::glue`])
/// and [`diff`] report a contract or lockfile violation as this type inside the returned
/// [`anyhow::Error`], so callers can tell the two classes apart with [`is_contract_error`]; the
/// `witforge` CLI does the same for `check` and `lock`, and exits with `1` for a violation and
/// `2` for any other error. The lower-level
/// [`lock::WireLock`] methods return plain errors, which their callers classify.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContractError {
    message: String,
}

impl ContractError {
    /// A violation described by `message`.
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    /// Classify `error` as a violation, keeping its whole context chain in the message.
    #[must_use]
    pub fn from_error(error: &anyhow::Error) -> Self {
        Self::new(format!("{error:#}"))
    }
}

impl fmt::Display for ContractError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)
    }
}

impl std::error::Error for ContractError {}

/// Whether `error`, or any error in its context chain, is a [`ContractError`].
#[must_use]
pub fn is_contract_error(error: &anyhow::Error) -> bool {
    error
        .chain()
        .any(<dyn std::error::Error>::is::<ContractError>)
}

/// An interface explicitly imported by a root world: `(Option<(namespace, package)>, interface)`.
type ExplicitWorldImport = (Option<(String, String)>, String);
/// A foreign world referenced by a root world's `include`: `(namespace, package, world_name)`.
type ForeignWorldInclude = (String, String, String);

/// A parsed WIT package together with the resolve that owns it.
///
/// The fields are crate-private so that `wit-parser` types do not become part of witforge's
/// public API. Callers use [`load`] and the model/subset/lock modules instead.
///
/// The boundary is compile-time enforced; external code cannot reach the resolved package:
///
/// ```compile_fail
/// let contract = witforge::load(".").unwrap();
/// let _ = contract.resolve;
/// ```
#[derive(Debug)]
pub struct Contract {
    pub(crate) resolve: Resolve,
    pub(crate) package: PackageId,
    pub(crate) dependencies: Vec<PackageId>,
    pub(crate) dependency_entries: BTreeMap<PackageId, PathBuf>,
    pub(crate) explicit_world_imports: BTreeMap<String, BTreeSet<ExplicitWorldImport>>,
    pub(crate) included_foreign_worlds: BTreeMap<String, Vec<ForeignWorldInclude>>,
}

impl Contract {
    /// Whether `package` is one of the standard `deps/` packages, rather than the root
    /// package or a nested package block in the root's source.
    #[must_use]
    pub(crate) fn is_dependency(&self, package: PackageId) -> bool {
        self.dependencies.contains(&package)
    }

    /// The `deps/` entry path that supplied `package`, when known.
    #[must_use]
    pub(crate) fn dependency_entry(&self, package: PackageId) -> Option<&Path> {
        self.dependency_entries.get(&package).map(PathBuf::as_path)
    }

    /// Whether `interface_id` was explicitly listed as an import of root world `world_name`
    /// (including through a local `include`) rather than synthesized by `wit-parser` world
    /// elaboration.
    #[must_use]
    pub(crate) fn is_explicit_world_import(
        &self,
        world_name: &str,
        interface_id: InterfaceId,
    ) -> bool {
        let interface = &self.resolve.interfaces[interface_id];
        let Some(name) = interface.name.as_deref() else {
            return true;
        };
        let package_key = interface
            .package
            .filter(|package| *package != self.package)
            .map(|package| {
                let pkg = &self.resolve.packages[package].name;
                (pkg.namespace.clone(), pkg.name.clone())
            });
        self.explicit_world_imports
            .get(world_name)
            .is_some_and(|imports| imports.contains(&(package_key, name.to_string())))
    }

    /// Foreign worlds reached by `include` from root world `world_name`.
    #[must_use]
    pub(crate) fn included_foreign_worlds(&self, world_name: &str) -> &[ForeignWorldInclude] {
        self.included_foreign_worlds
            .get(world_name)
            .map_or(&[], Vec::as_slice)
    }
}

/// Load the WIT package in `dir`.
///
/// Files in `dir` and in each package directory under `deps/` are loaded in ascending file-name
/// byte order (never raw directory-enumeration order or locale order), so source spans and model
/// ordering are deterministic across file systems and platforms. All features are enabled, so
/// items gated by `@unstable` are kept; otherwise they would silently disappear and the lockfile
/// would report their members as removed.
///
/// # Errors
///
/// Returns an error when the directory cannot be read or the WIT does not parse or resolve, and a
/// [`ContractError`] when the dependency graph pins more than one version of one package or a
/// dependency uses the root package's namespace and name.
pub fn load(dir: &Path) -> Result<Contract> {
    let load_context = || format!("failed to load the WIT package in {}", dir.display());
    let top_pkg = parse_sorted_dir(dir)
        .with_context(|| format!("failed to parse package: {}", dir.display()))
        .with_context(load_context)?;
    let (explicit_world_imports, included_foreign_worlds) =
        collect_explicit_world_imports(&top_pkg.main);

    let mut resolve = Resolve {
        all_features: true,
        ..Resolve::default()
    };
    let loaded_deps = load_deps_dir(&dir.join("deps"), &mut resolve).with_context(load_context)?;

    // Check dependency identity before `push_groups` so duplicate package names become a
    // `ContractError` (exit 1) naming the package, versions, and `deps/` entries rather than a
    // `wit-parser` duplicate-package error (exit 2).
    crate::model::ensure_dependency_identity(&top_pkg.main.name, &loaded_deps.scanned)?;

    let package = resolve
        .push_groups(top_pkg, loaded_deps.groups)
        .map_err(|err| {
            let err: anyhow::Error = err.into();
            anyhow!("{}", resolve.render_error(&err))
        })
        .with_context(load_context)?;

    let mut dependencies = Vec::new();
    let mut dependency_entries = BTreeMap::new();
    for (id, pkg) in &resolve.packages {
        if id == package {
            continue;
        }
        if let Some(entry) = loaded_deps
            .binary_entries
            .get(&id)
            .cloned()
            .or_else(|| loaded_deps.entries_by_package.get(&pkg.name).cloned())
        {
            dependencies.push(id);
            dependency_entries.insert(id, entry);
        }
    }
    Ok(Contract {
        resolve,
        package,
        dependencies,
        dependency_entries,
        explicit_world_imports,
        included_foreign_worlds,
    })
}

struct LoadedDeps {
    groups: Vec<UnresolvedPackageGroup>,
    entries_by_package: BTreeMap<PackageName, PathBuf>,
    binary_entries: BTreeMap<PackageId, PathBuf>,
    scanned: Vec<(PackageName, PathBuf)>,
}

fn load_deps_dir(deps_dir: &Path, resolve: &mut Resolve) -> Result<LoadedDeps> {
    let mut loaded = LoadedDeps {
        groups: Vec::new(),
        entries_by_package: BTreeMap::new(),
        binary_entries: BTreeMap::new(),
        scanned: Vec::new(),
    };
    if !deps_dir.exists() {
        return Ok(loaded);
    }
    let dep_paths = sorted_directory_entries(deps_dir).with_context(|| {
        format!(
            "failed to parse dependency directory: {}",
            deps_dir.display()
        )
    })?;
    for entry_path in dep_paths {
        let file_type = fs::symlink_metadata(&entry_path)
            .with_context(|| format!("failed to inspect {}", entry_path.display()))?
            .file_type();
        let is_dir = file_type.is_dir()
            || (file_type.is_symlink() && fs::metadata(&entry_path).is_ok_and(|m| m.is_dir()));
        if is_dir {
            let group = parse_sorted_dir(&entry_path)
                .with_context(|| format!("failed to parse package: {}", entry_path.display()))?;
            for pkg in std::iter::once(&group.main).chain(&group.nested) {
                loaded.scanned.push((pkg.name.clone(), entry_path.clone()));
                loaded
                    .entries_by_package
                    .insert(pkg.name.clone(), entry_path.clone());
            }
            loaded.groups.push(group);
        } else {
            match entry_path.extension().and_then(std::ffi::OsStr::to_str) {
                Some("wit") => {
                    let group = parse_single_wit_file(&entry_path)?;
                    for pkg in std::iter::once(&group.main).chain(&group.nested) {
                        loaded.scanned.push((pkg.name.clone(), entry_path.clone()));
                        loaded
                            .entries_by_package
                            .insert(pkg.name.clone(), entry_path.clone());
                    }
                    loaded.groups.push(group);
                }
                Some("wat" | "wasm") => {
                    let before: BTreeSet<PackageId> =
                        resolve.packages.iter().map(|(id, _)| id).collect();
                    resolve.push_file(&entry_path).with_context(|| {
                        format!("failed to parse package: {}", entry_path.display())
                    })?;
                    for (id, pkg) in &resolve.packages {
                        if !before.contains(&id) {
                            loaded.scanned.push((pkg.name.clone(), entry_path.clone()));
                            loaded.binary_entries.insert(id, entry_path.clone());
                        }
                    }
                }
                _ => {}
            }
        }
    }
    Ok(loaded)
}

/// Record the interface imports and foreign world includes explicitly written in each root world
/// (including through local `include`s) before `wit-parser` world elaboration synthesizes
/// implicit dependency imports.
#[allow(clippy::type_complexity)]
fn collect_explicit_world_imports(
    package: &UnresolvedPackage,
) -> (
    BTreeMap<String, BTreeSet<ExplicitWorldImport>>,
    BTreeMap<String, Vec<ForeignWorldInclude>>,
) {
    let mut foreign_interfaces: BTreeMap<InterfaceId, ((String, String), String)> = BTreeMap::new();
    let mut foreign_worlds: BTreeMap<WorldId, ForeignWorldInclude> = BTreeMap::new();
    for (pkg_name, items) in &package.foreign_deps {
        for (item_name, (ast_item, _)) in items {
            match ast_item {
                AstItem::Interface(interface_id) => {
                    foreign_interfaces.insert(
                        *interface_id,
                        (
                            (pkg_name.namespace.clone(), pkg_name.name.clone()),
                            item_name.clone(),
                        ),
                    );
                }
                AstItem::World(world_id) => {
                    foreign_worlds.insert(
                        *world_id,
                        (
                            pkg_name.namespace.clone(),
                            pkg_name.name.clone(),
                            item_name.clone(),
                        ),
                    );
                }
            }
        }
    }
    let mut imports_by_world = BTreeMap::new();
    let mut foreign_includes_by_world = BTreeMap::new();
    for (world_id, world) in &package.worlds {
        if foreign_worlds.contains_key(&world_id) {
            continue;
        }
        let mut imports = BTreeSet::new();
        let mut included = Vec::new();
        let mut visited = BTreeSet::new();
        collect_world_explicit_imports(
            package,
            world_id,
            &foreign_interfaces,
            &foreign_worlds,
            &mut visited,
            &mut imports,
            &mut included,
        );
        imports_by_world.insert(world.name.clone(), imports);
        if !included.is_empty() {
            foreign_includes_by_world.insert(world.name.clone(), included);
        }
    }
    (imports_by_world, foreign_includes_by_world)
}

fn collect_world_explicit_imports(
    package: &UnresolvedPackage,
    world_id: WorldId,
    foreign_interfaces: &BTreeMap<InterfaceId, ((String, String), String)>,
    foreign_worlds: &BTreeMap<WorldId, ForeignWorldInclude>,
    visited: &mut BTreeSet<WorldId>,
    out_imports: &mut BTreeSet<ExplicitWorldImport>,
    out_foreign_worlds: &mut Vec<ForeignWorldInclude>,
) {
    if !visited.insert(world_id) {
        return;
    }
    let world = &package.worlds[world_id];
    for key in world.imports.keys() {
        if let WorldKey::Interface(interface_id) = key {
            if let Some((pkg, name)) = foreign_interfaces.get(interface_id) {
                out_imports.insert((Some(pkg.clone()), name.clone()));
            } else if let Some(name) = package.interfaces[*interface_id].name.as_ref() {
                out_imports.insert((None, name.clone()));
            }
        }
    }
    for include in &world.includes {
        if let Some(foreign) = foreign_worlds.get(&include.id) {
            if !out_foreign_worlds.contains(foreign) {
                out_foreign_worlds.push(foreign.clone());
            }
        } else {
            collect_world_explicit_imports(
                package,
                include.id,
                foreign_interfaces,
                foreign_worlds,
                visited,
                out_imports,
                out_foreign_worlds,
            );
        }
    }
}

/// Sort paths by their file name as raw bytes (UTF-8 bytes for valid UTF-8 names), never by
/// locale or OS directory-enumeration order.
fn sort_paths_by_filename_bytes(paths: &mut [PathBuf]) {
    paths.sort_by(|left, right| {
        let left_name = left
            .file_name()
            .map(std::ffi::OsStr::as_encoded_bytes)
            .unwrap_or_default();
        let right_name = right
            .file_name()
            .map(std::ffi::OsStr::as_encoded_bytes)
            .unwrap_or_default();
        left_name.cmp(right_name).then_with(|| {
            left.as_os_str()
                .as_encoded_bytes()
                .cmp(right.as_os_str().as_encoded_bytes())
        })
    });
}

fn sorted_directory_entries(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut paths = Vec::new();
    for entry in
        fs::read_dir(dir).with_context(|| format!("failed to read directory {}", dir.display()))?
    {
        let entry = entry.with_context(|| format!("failed to read directory {}", dir.display()))?;
        paths.push(entry.path());
    }
    sort_paths_by_filename_bytes(&mut paths);
    Ok(paths)
}

fn sorted_wit_files(dir: &Path) -> Result<Vec<PathBuf>> {
    let mut files = Vec::new();
    for entry_path in sorted_directory_entries(dir)? {
        let ty = fs::symlink_metadata(&entry_path)
            .with_context(|| format!("failed to inspect {}", entry_path.display()))?
            .file_type();
        if ty.is_dir() || (ty.is_symlink() && entry_path.is_dir()) {
            continue;
        }
        if entry_path.extension().and_then(std::ffi::OsStr::to_str) != Some("wit") {
            continue;
        }
        files.push(entry_path);
    }
    Ok(files)
}

fn parse_sorted_dir(dir: &Path) -> Result<UnresolvedPackageGroup> {
    let mut map = SourceMap::new();
    for file in sorted_wit_files(dir)? {
        map.push_file(&file)?;
    }
    map.parse()
        .map_err(|(source_map, err)| anyhow!("{}", err.render(&source_map)))
}

fn parse_single_wit_file(path: &Path) -> Result<UnresolvedPackageGroup> {
    let mut map = SourceMap::new();
    map.push_file(path)?;
    map.parse()
        .map_err(|(source_map, err)| anyhow!("{}", err.render(&source_map)))
        .with_context(|| format!("failed to parse package: {}", path.display()))
}

/// Refuse two models whose pinned dependency graphs cannot share one generated binding surface.
///
/// This is the comparison the generators and `diff` apply. `check --against` uses it when the
/// baseline lockfile belongs to a version directory, so a dependency change cannot pass the
/// lockfile gate while every generator refuses the root.
///
/// # Errors
///
/// Returns a [`ContractError`] for the first changed package, version, interface, type,
/// function, declaration order, stability metadata, or docs.
pub fn ensure_same_dependency_graph(old: &model::Model, new: &model::Model) -> Result<()> {
    crate::emit::versions::ensure_same_dependency_graph(&old.dependencies, &new.dependencies)
}

/// The default released-version manifest name.
pub const RELEASED_MANIFEST: &str = "released";

/// The validated contents of a `wit/released` manifest.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReleasedManifest {
    directories: Vec<String>,
}

impl ReleasedManifest {
    /// Parse and validate a released manifest.
    ///
    /// Entries are plain directory names relative to the manifest's directory: an ASCII letter
    /// or digit followed by ASCII letters, digits, `.`, `_`, or `-`
    /// (`^[A-Za-z0-9][A-Za-z0-9._-]*$`). That excludes `.`, `..`, hidden names, whitespace,
    /// separators, and absolute paths. Leading and trailing ASCII whitespace (space, tab, CR,
    /// VT, FF) is trimmed; blank lines and `#` comments are ignored. Invalid, duplicate,
    /// missing, symlinked, and lockfile-less directories are errors, as is any symlink inside a
    /// listed directory.
    ///
    /// # Errors
    ///
    /// Returns an error for an unreadable manifest, any invalid entry, or a symlinked directory
    /// or directory content.
    pub fn load(path: &Path) -> Result<Self> {
        let text = fs::read_to_string(path)
            .with_context(|| format!("failed to read released manifest {}", path.display()))?;
        let parent = path.parent().unwrap_or_else(|| Path::new("."));
        let mut directories = Vec::new();
        let mut seen = BTreeSet::new();
        for (index, raw) in text.lines().enumerate() {
            let line_no = index + 1;
            let line = raw.trim_matches([' ', '\t', '\r', '\x0b', '\x0c']);
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if !plain_directory_name(line) {
                bail!(
                    "line {line_no}: released entry `{line}` is not a plain directory name \
                     (`^[A-Za-z0-9][A-Za-z0-9._-]*$`)"
                );
            }
            if !seen.insert(line.to_string()) {
                bail!("line {line_no}: duplicate released entry `{line}`");
            }
            let directory = parent.join(line);
            if fs::symlink_metadata(&directory)
                .is_ok_and(|metadata| metadata.file_type().is_symlink())
            {
                bail!(
                    "line {line_no}: released directory `{line}` is a symlink; witforge refuses \
                     symlinked version directories"
                );
            }
            if !directory.is_dir() {
                bail!("line {line_no}: released directory `{line}` does not exist");
            }
            if !directory.join("wire.lock").is_file() {
                bail!("line {line_no}: released directory `{line}` has no wire.lock");
            }
            ensure_no_symlinks(&directory, line)?;
            directories.push(line.to_string());
        }
        Ok(Self { directories })
    }

    /// The released directory names in manifest order.
    #[must_use]
    pub fn directories(&self) -> &[String] {
        &self.directories
    }

    /// Whether `directory` is listed.
    #[must_use]
    pub fn contains(&self, directory: &str) -> bool {
        self.directories.iter().any(|entry| entry == directory)
    }
}

/// Whether `name` matches `^[A-Za-z0-9][A-Za-z0-9._-]*$`, the released-manifest entry rule shared
/// with `ci/check-release-freeze.sh`.
fn plain_directory_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    bytes
        .next()
        .is_some_and(|first| first.is_ascii_alphanumeric())
        && bytes.all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
}

/// Refuse any symlink inside a released version directory.
///
/// The freeze script checks the tracked history; this check also covers a working tree that was
/// modified after checkout. A released directory's contract, lockfile, and dependency files must
/// be the released bytes, not a link whose target can change outside the manifest's view.
fn ensure_no_symlinks(directory: &Path, entry: &str) -> Result<()> {
    for child in fs::read_dir(directory)
        .with_context(|| format!("failed to read released directory `{entry}`"))?
    {
        let child =
            child.with_context(|| format!("failed to read released directory `{entry}`"))?;
        let file_type = child
            .file_type()
            .with_context(|| format!("failed to inspect `{}`", child.path().display()))?;
        if file_type.is_symlink() {
            bail!(
                "released directory `{entry}` contains symlink `{}`; a released version must \
                 contain regular files and directories",
                child.path().display()
            );
        }
        if file_type.is_dir() {
            ensure_no_symlinks(&child.path(), entry)?;
        }
    }
    Ok(())
}

/// The default manifest path for a version directory: `<wit-dir>/../released`.
///
/// The version directory is canonicalised first, so `.` and a symlinked WIT root name the
/// directory's real parent. A directory that cannot be resolved is used as given.
#[must_use]
pub fn default_released_manifest(wit_dir: &Path) -> PathBuf {
    let resolved = fs::canonicalize(wit_dir).unwrap_or_else(|_| wit_dir.to_path_buf());
    resolved
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .join(RELEASED_MANIFEST)
}

/// Refuse a version directory that is a symlink or not a directory.
///
/// witforge never follows a symlinked version directory: a released version reached through a
/// link could change through the link's target, which neither the released manifest beside the
/// link nor `ci/check-release-freeze.sh` would see. The WIT root and its parents may be symlinks.
/// Trailing separators and `.` components are ignored, so `v0.1/` is checked as `v0.1`.
///
/// # Errors
///
/// Returns an error when the directory is a symlink, is missing, or is not a directory.
pub fn ensure_version_directory(wit_dir: &Path) -> Result<()> {
    let path: PathBuf = wit_dir.components().collect();
    let metadata = fs::symlink_metadata(&path)
        .with_context(|| format!("failed to read version directory {}", wit_dir.display()))?;
    if metadata.file_type().is_symlink() {
        bail!(
            "version directory `{}` is a symlink; witforge refuses symlinked version \
             directories, so a released version cannot change through its link's target",
            wit_dir.display()
        );
    }
    if !metadata.is_dir() {
        bail!(
            "version directory `{}` is not a directory",
            wit_dir.display()
        );
    }
    Ok(())
}

/// Refuse a version directory whose name spells another version than its package declares.
///
/// A directory named `v<major>.<minor>` must hold the package version `<major>.<minor>.0`, as
/// the generators require of every version directory; a directory with any other name is an
/// ad-hoc directory, such as a test fixture, and may hold any version. The name is read from
/// the resolved directory, so `.` stands for the directory it is.
///
/// # Errors
///
/// Returns a [`ContractError`] when the package version differs from the version the name
/// spells, and a plain error when the directory cannot be resolved or its name spells a version
/// with a sign or leading zeros (`v+0.1`, `v00.02`), which the generators refuse too.
pub fn ensure_directory_version(wit_dir: &Path, model: &model::Model) -> Result<()> {
    let resolved = fs::canonicalize(wit_dir)
        .with_context(|| format!("failed to resolve version directory {}", wit_dir.display()))?;
    let Some(name) = resolved.file_name().map(|name| name.to_string_lossy()) else {
        return Ok(());
    };
    let Some((major, minor)) = parse_version_directory(&name)? else {
        return Ok(());
    };
    let version = model.package.version;
    if version.major != major || version.minor != minor || version.patch != 0 {
        bail!(ContractError::new(format!(
            "version directory {} declares package version {version}, expected \
             {major}.{minor}.0; a new version is a new `v<major>.<minor>` directory",
            wit_dir.display()
        )));
    }
    Ok(())
}

/// Load the manifest for a version directory.
///
/// A missing default manifest means no versions are released. An explicitly supplied missing
/// manifest is an error.
///
/// # Errors
///
/// Returns an error for an invalid manifest or an explicitly missing manifest.
pub fn load_released_manifest(wit_dir: &Path, explicit: Option<&Path>) -> Result<ReleasedManifest> {
    load_manifest_or_default(default_released_manifest(wit_dir), explicit)
}

/// Load the manifest for a WIT root, as the generators do; the default is `<wit-root>/released`.
///
/// A missing default manifest means no versions are released. An explicitly supplied missing
/// manifest is an error.
///
/// # Errors
///
/// Returns an error for an invalid manifest or an explicitly missing manifest.
pub fn load_root_released_manifest(
    wit_root: &Path,
    explicit: Option<&Path>,
) -> Result<ReleasedManifest> {
    load_manifest_or_default(wit_root.join(RELEASED_MANIFEST), explicit)
}

fn load_manifest_or_default(default: PathBuf, explicit: Option<&Path>) -> Result<ReleasedManifest> {
    let path = explicit.map_or(default, Path::to_path_buf);
    if explicit.is_none() && !path.exists() {
        return Ok(ReleasedManifest::default());
    }
    ReleasedManifest::load(&path)
}

/// Refuse to lock a version directory listed in `wit/released`, or one that is a symlink.
///
/// The decision compares resolved paths, not names: `wit_dir` and every listed directory are
/// canonicalised, so `.` and `..` components and a symlinked WIT root cannot hide a released
/// directory. The `released` manifest beside the directory always applies, read both where the
/// directory is written (its lexical parent) and where it resolves to (its canonical parent); an
/// `explicit` manifest is read as well, never instead.
///
/// # Errors
///
/// Returns an error when the directory is a symlink, cannot be resolved, or is released, or a
/// manifest is invalid.
pub fn ensure_not_released(wit_dir: &Path, explicit: Option<&Path>) -> Result<()> {
    ensure_version_directory(wit_dir)?;
    let directory = fs::canonicalize(wit_dir)
        .with_context(|| format!("failed to resolve version directory {}", wit_dir.display()))?;
    if let Some(released) = released_directories(&[wit_dir], explicit)?
        .into_iter()
        .find(|released| released.directory == directory)
    {
        bail!(
            "version directory `{}` is released (listed as `{}` in {}) and cannot be locked",
            wit_dir.display(),
            released.entry,
            released.manifest.display()
        );
    }
    Ok(())
}

/// Whether `wit_dir` is listed in the manifests [`ensure_not_released`] reads.
///
/// # Errors
///
/// Returns an error when the directory cannot be resolved or a manifest is invalid.
pub fn is_released(wit_dir: &Path, explicit: Option<&Path>) -> Result<bool> {
    let directory = fs::canonicalize(wit_dir)
        .with_context(|| format!("failed to resolve version directory {}", wit_dir.display()))?;
    Ok(released_directories(&[wit_dir], explicit)?
        .iter()
        .any(|released| released.directory == directory))
}

/// Refuse to write a lockfile into a released version directory, as [`ensure_not_released`]
/// refuses to lock one: `lock --lock <path>` must not rewrite a released `wire.lock`.
///
/// The lockfile's directory is canonicalised and compared with every directory released by the
/// manifests [`ensure_not_released`] reads for `wit_dir`, by the `released` manifest beside the
/// lockfile's own directory, and by the one beside each of its ancestors that is named like a
/// version directory (`v<major>.<minor>`), each ancestor taken both as written and as resolved.
/// So neither a lockfile in another WIT root, nor one in a subdirectory of a released directory,
/// nor an `explicit` manifest that omits the directory gets past the manifest that lists it.
///
/// # Errors
///
/// Returns an error when the lockfile's directory cannot be resolved, is or is inside a
/// released directory, or a manifest is invalid.
pub fn ensure_lockfile_not_released(
    lock: &Path,
    wit_dir: &Path,
    explicit: Option<&Path>,
) -> Result<()> {
    let parent = lock
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let resolved = fs::canonicalize(parent).with_context(|| {
        format!(
            "failed to resolve the directory of lockfile {}",
            lock.display()
        )
    })?;
    let ancestors = version_ancestors(parent, &resolved)?;
    let mut directories = vec![wit_dir, parent];
    directories.extend(ancestors.iter().map(PathBuf::as_path));
    if let Some(released) = released_directories(&directories, explicit)?
        .into_iter()
        .find(|released| resolved.starts_with(&released.directory))
    {
        bail!(
            "lockfile `{}` is in the released directory `{}` (listed as `{}` in {}) and \
             cannot be written",
            lock.display(),
            released.directory.display(),
            released.entry,
            released.manifest.display()
        );
    }
    Ok(())
}

/// The ancestors of a directory, `written` (made absolute lexically) and `resolved`, that are
/// named like a version directory: `v`, ASCII digits, `.`, ASCII digits. Leading zeros are
/// accepted, so a non-canonical spelling of a listed directory is not passed over.
fn version_ancestors(written: &Path, resolved: &Path) -> Result<Vec<PathBuf>> {
    let written = lexical_absolute(written)?;
    let mut found: Vec<PathBuf> = Vec::new();
    for ancestor in written.ancestors().chain(resolved.ancestors()) {
        let versioned = ancestor
            .file_name()
            .and_then(std::ffi::OsStr::to_str)
            .and_then(|name| name.strip_prefix('v'))
            .and_then(|rest| rest.split_once('.'))
            .is_some_and(|(major, minor)| {
                [major, minor].iter().all(|digits| {
                    !digits.is_empty() && digits.bytes().all(|byte| byte.is_ascii_digit())
                })
            });
        if versioned && !found.iter().any(|known| known == ancestor) {
            found.push(ancestor.to_path_buf());
        }
    }
    Ok(found)
}

/// A directory a released manifest lists, resolved.
struct ReleasedDirectory {
    /// The canonical directory.
    directory: PathBuf,
    /// The manifest entry that names it.
    entry: String,
    /// The manifest that lists it.
    manifest: PathBuf,
}

/// Every directory released by the `explicit` manifest and by the default manifest beside each
/// of `directories`, at its lexical and its canonical location. A missing default manifest
/// releases nothing.
fn released_directories(
    directories: &[&Path],
    explicit: Option<&Path>,
) -> Result<Vec<ReleasedDirectory>> {
    let mut manifests: Vec<PathBuf> = explicit.iter().map(|path| path.to_path_buf()).collect();
    for directory in directories {
        let mut defaults = vec![default_released_manifest(directory)];
        if let Some(parent) = lexical_absolute(directory)?.parent() {
            defaults.push(parent.join(RELEASED_MANIFEST));
        }
        for candidate in defaults {
            if candidate.exists() && !manifests.contains(&candidate) {
                manifests.push(candidate);
            }
        }
    }
    let mut released = Vec::new();
    for path in manifests {
        let manifest = ReleasedManifest::load(&path)?;
        let base = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        for entry in manifest.directories() {
            let listed = base.join(entry);
            let directory = fs::canonicalize(&listed).with_context(|| {
                format!("failed to resolve released directory {}", listed.display())
            })?;
            released.push(ReleasedDirectory {
                directory,
                entry: entry.clone(),
                manifest: path.clone(),
            });
        }
    }
    Ok(released)
}

/// `path` made absolute with its `.` and `..` components removed lexically, as written rather
/// than as the file system resolves it.
fn lexical_absolute(path: &Path) -> Result<PathBuf> {
    let absolute = std::path::absolute(path)
        .with_context(|| format!("failed to resolve {}", path.display()))?;
    let mut normalized = PathBuf::new();
    for component in absolute.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            other => normalized.push(other),
        }
    }
    Ok(normalized)
}

/// The version a `v<major>.<minor>` directory name spells, or `None` for any other name.
///
/// Both numbers must be ASCII digits without a sign or leading zeros. A name the numbers parse
/// from but that spells them otherwise (`v+0.1`, `v00.02`) is an error rather than a skipped
/// directory, so no version silently drops out of the generated output.
pub(crate) fn parse_version_directory(name: &str) -> Result<Option<(u64, u64)>> {
    let Some((major, minor)) = name.strip_prefix('v').and_then(|rest| rest.split_once('.')) else {
        return Ok(None);
    };
    let (Ok(major_number), Ok(minor_number)) = (major.parse::<u64>(), minor.parse::<u64>()) else {
        return Ok(None);
    };
    if !canonical_number(major) || !canonical_number(minor) {
        bail!(
            "version directory `{name}` must be spelled `v{major_number}.{minor_number}`: ASCII \
             digits without a sign or leading zeros"
        );
    }
    Ok(Some((major_number, minor_number)))
}

fn canonical_number(digits: &str) -> bool {
    !digits.is_empty()
        && digits.bytes().all(|byte| byte.is_ascii_digit())
        && (digits == "0" || !digits.starts_with('0'))
}

/// Format `path` as one POSIX shell word that witforge reads back as the same path.
///
/// Unchanged when it holds only characters no shell treats specially, and single-quoted
/// otherwise. A relative path starting with `-` gets a leading `./` so the command line does not
/// read it as an option.
pub(crate) fn shell_quote(path: &Path) -> String {
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

/// The highest sibling `v<major>.<minor>` version directory of `dir` in the same major whose
/// `wire.lock` exists, if any.
pub(crate) fn sibling_predecessor(dir: &Path) -> Option<(u64, u64, PathBuf)> {
    let canonical = fs::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    let name = canonical.file_name()?.to_str()?;
    let (major, minor) = parse_version_directory(name).ok()??;
    let parent = dir.parent().filter(|parent| !parent.as_os_str().is_empty());
    let scan_parent = parent.or_else(|| canonical.parent())?;
    let mut best: Option<(u64, PathBuf)> = None;
    for entry in fs::read_dir(scan_parent).ok()?.flatten() {
        let entry_path = entry.path();
        if !entry_path.is_dir() {
            continue;
        }
        let Some(entry_name) = entry_path.file_name().and_then(std::ffi::OsStr::to_str) else {
            continue;
        };
        let Ok(Some((sibling_major, sibling_minor))) = parse_version_directory(entry_name) else {
            continue;
        };
        if sibling_major == major
            && sibling_minor < minor
            && best
                .as_ref()
                .is_none_or(|(best_minor, _)| sibling_minor > *best_minor)
        {
            best = Some((sibling_minor, entry_path));
        }
    }
    let (prev_minor, prev_path) = best?;
    Some((major, prev_minor, prev_path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sort_paths_by_filename_bytes_is_deterministic_byte_order() {
        let mut shuffled = vec![
            PathBuf::from("dir/z.wit"),
            PathBuf::from("dir/b.wit"),
            PathBuf::from("dir/B.wit"),
            PathBuf::from("dir/a.wit"),
            PathBuf::from("dir/10.wit"),
            PathBuf::from("dir/2.wit"),
            PathBuf::from("dir/é.wit"),
        ];
        sort_paths_by_filename_bytes(&mut shuffled);
        let names: Vec<&str> = shuffled
            .iter()
            .map(|p| p.file_name().unwrap().to_str().unwrap())
            .collect();
        // Raw UTF-8 byte order: digits ('1' < '2'), uppercase ('B'), lowercase ('a' < 'b' < 'z'),
        // then non-ASCII UTF-8 bytes (0xc3 ...).
        assert_eq!(
            names,
            vec![
                "10.wit", "2.wit", "B.wit", "a.wit", "b.wit", "z.wit", "é.wit"
            ]
        );
    }

    #[test]
    fn sorted_load_preserves_source_positions_for_every_file() {
        let dir =
            std::env::temp_dir().join(format!("witforge-lib-sorted-spans-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        // Write `z.wit` before `a.wit`; `a.wit` must still receive earlier spans.
        fs::write(
            dir.join("z.wit"),
            "package ex:spans@0.1.0;\n\ninterface zeta {\n    record z { id: u32 }\n}\n",
        )
        .unwrap();
        fs::write(
            dir.join("a.wit"),
            "interface alpha {\n    record a { id: u32 }\n}\n",
        )
        .unwrap();
        let contract = load(&dir).unwrap();
        let pkg = &contract.resolve.packages[contract.package];
        let alpha_span = contract.resolve.interfaces[pkg.interfaces["alpha"]].span;
        let zeta_span = contract.resolve.interfaces[pkg.interfaces["zeta"]].span;
        assert!(alpha_span.start() < zeta_span.start());
        let alpha_loc = contract
            .resolve
            .source_map
            .resolve_span(alpha_span)
            .unwrap();
        let zeta_loc = contract.resolve.source_map.resolve_span(zeta_span).unwrap();
        assert!(alpha_loc.path.ends_with("a.wit"), "{}", alpha_loc.path);
        assert!(zeta_loc.path.ends_with("z.wit"), "{}", zeta_loc.path);
        let alpha_rendered = contract.resolve.render_location(alpha_span);
        let zeta_rendered = contract.resolve.render_location(zeta_span);
        assert!(alpha_rendered.contains("a.wit:1:"), "{alpha_rendered}");
        assert!(zeta_rendered.contains("z.wit:3:"), "{zeta_rendered}");
        fs::remove_dir_all(&dir).unwrap();
    }
}

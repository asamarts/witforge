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

use anyhow::{Context, Result, bail};
use wit_parser::{
    AstItem, InterfaceId, PackageId, PackageName, Resolve, UnresolvedPackage,
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
}

/// Load the WIT package in `dir`.
///
/// All features are enabled, so items gated by `@unstable` are kept; otherwise they would
/// silently disappear and the lockfile would report their members as removed.
///
/// # Errors
///
/// Returns an error when the directory cannot be read or the WIT does not parse or resolve, and a
/// [`ContractError`] when the dependency graph pins more than one version of one package or a
/// dependency uses the root package's namespace and name.
pub fn load(dir: &Path) -> Result<Contract> {
    // `wit-parser` refuses two identical package names before witforge can classify the error.
    // Scan the dependency entries first so that every duplicate-package shape is a contract
    // violation (exit 1) that names the package, versions, and entries.
    let root_group = UnresolvedPackageGroup::parse_dir(dir).ok();
    let scanned = scan_dependency_packages(dir);
    if !scanned.is_empty()
        && let Some(root) = &root_group
    {
        crate::model::ensure_dependency_identity(&root.main.name, &scanned)?;
    }
    let explicit_world_imports = root_group
        .as_ref()
        .map(|group| collect_explicit_world_imports(&group.main))
        .unwrap_or_default();

    let mut resolve = Resolve {
        all_features: true,
        ..Resolve::default()
    };
    let (package, sources) = resolve
        .push_dir(dir)
        .with_context(|| format!("failed to load the WIT package in {}", dir.display()))?;
    let deps_dir = dir.join("deps");
    let dependencies: Vec<PackageId> = resolve
        .packages
        .iter()
        .map(|(id, _)| id)
        .filter(|id| *id != package)
        .filter(|id| {
            sources
                .package_paths(*id)
                .is_some_and(|mut paths| paths.all(|path| path.starts_with(&deps_dir)))
        })
        .collect();
    let dependency_entries_list: Vec<(PackageId, PathBuf)> = dependencies
        .iter()
        .filter_map(|id| {
            sources
                .package_paths(*id)
                .and_then(|mut paths| paths.next())
                .map(|source| (*id, dependency_entry_path(dir, source)))
        })
        .collect();
    let dependency_names: Vec<(PackageName, PathBuf)> = dependency_entries_list
        .iter()
        .map(|(id, entry)| (resolve.packages[*id].name.clone(), entry.clone()))
        .collect();
    crate::model::ensure_dependency_identity(&resolve.packages[package].name, &dependency_names)?;
    let dependency_entries = dependency_entries_list.into_iter().collect();
    Ok(Contract {
        resolve,
        package,
        dependencies,
        dependency_entries,
        explicit_world_imports,
    })
}

/// Record the interface imports explicitly written in each root world (including local `include`s)
/// before `wit-parser` world elaboration synthesizes implicit dependency imports.
fn collect_explicit_world_imports(
    package: &UnresolvedPackage,
) -> BTreeMap<String, BTreeSet<ExplicitWorldImport>> {
    let mut foreign_interfaces: BTreeMap<InterfaceId, ((String, String), String)> = BTreeMap::new();
    for (pkg_name, items) in &package.foreign_deps {
        for (iface_name, (ast_item, _)) in items {
            if let AstItem::Interface(interface_id) = ast_item {
                foreign_interfaces.insert(
                    *interface_id,
                    (
                        (pkg_name.namespace.clone(), pkg_name.name.clone()),
                        iface_name.clone(),
                    ),
                );
            }
        }
    }
    let mut result = BTreeMap::new();
    for (world_id, world) in &package.worlds {
        let mut imports = BTreeSet::new();
        let mut visited = BTreeSet::new();
        collect_world_explicit_imports(
            package,
            world_id,
            &foreign_interfaces,
            &mut visited,
            &mut imports,
        );
        result.insert(world.name.clone(), imports);
    }
    result
}

fn collect_world_explicit_imports(
    package: &UnresolvedPackage,
    world_id: WorldId,
    foreign_interfaces: &BTreeMap<InterfaceId, ((String, String), String)>,
    visited: &mut BTreeSet<WorldId>,
    out: &mut BTreeSet<ExplicitWorldImport>,
) {
    if !visited.insert(world_id) {
        return;
    }
    let world = &package.worlds[world_id];
    for key in world.imports.keys() {
        if let WorldKey::Interface(interface_id) = key {
            if let Some((pkg, name)) = foreign_interfaces.get(interface_id) {
                out.insert((Some(pkg.clone()), name.clone()));
            } else if let Some(name) = package.interfaces[*interface_id].name.as_ref() {
                out.insert((None, name.clone()));
            }
        }
    }
    for include in &world.includes {
        // `include.id` is in `package.worlds` for both local and foreign world stubs; a foreign
        // stub has no imports here, while a local world contributes its own explicit imports.
        collect_world_explicit_imports(package, include.id, foreign_interfaces, visited, out);
    }
}

/// The `deps/` entry that contains a dependency source path.
fn dependency_entry_path(dir: &Path, source: &Path) -> PathBuf {
    let deps = dir.join("deps");
    source
        .strip_prefix(&deps)
        .ok()
        .and_then(|rest| rest.components().next())
        .map_or_else(
            || source.to_path_buf(),
            |component| deps.join(component.as_os_str()),
        )
}

/// Parse the package declarations in `deps/` without resolving them.
///
/// This only supplies names for the duplicate-package diagnostics; `wit-parser` still owns the
/// real parse and reports its own diagnostics for a malformed entry. Entries it cannot read or
/// parse are skipped here and refused by `push_dir` afterwards.
fn scan_dependency_packages(dir: &Path) -> Vec<(PackageName, PathBuf)> {
    let deps = dir.join("deps");
    let Ok(entries) = fs::read_dir(&deps) else {
        return Vec::new();
    };
    let mut entries: Vec<_> = entries.flatten().collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    let mut packages = Vec::new();
    for entry in entries {
        let path = entry.path();
        let group = if path.is_dir() {
            UnresolvedPackageGroup::parse_dir(&path).ok()
        } else if path
            .extension()
            .and_then(std::ffi::OsStr::to_str)
            .is_some_and(|extension| extension.eq_ignore_ascii_case("wit"))
        {
            fs::read_to_string(&path)
                .ok()
                .and_then(|text| UnresolvedPackageGroup::parse(&path, &text).ok())
        } else {
            None
        };
        let Some(group) = group else {
            continue;
        };
        for package in std::iter::once(group.main).chain(group.nested) {
            packages.push((package.name, path.clone()));
        }
    }
    packages
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

//! The restricted WIT subset that every contract must stay inside.
//!
//! Version 1 of the subset is pure and synchronous, and maps cleanly onto Rust
//! and protobuf: no resources or handles, no `future`, `stream`, or `error-context`, no tuples,
//! no `async` functions or `get`/`set` accessors. Primitives include `char`; containers include
//! `list<T>`, fixed-length `list<T, N>` (`1 <= N <= 65536`), `option<T>`, `result<T, E>`, and
//! `map<K, V>` with `bool`, integer, `char`, or `string` keys.
//! Types and functions live in the package's named interfaces, and a local type may reference a
//! named type from a standard `deps/` package. Worlds may import and export interfaces defined
//! by this package or by a standard `deps/` package without renaming them or defining them
//! inline. Only `@since`, `@unstable`, and `@deprecated` annotations are accepted. Names in one
//! scope must be distinct ASCII case-insensitively. Anything the checker does not recognise is
//! refused rather than passed through, so a new `wit-parser` construct cannot slip into the
//! generated bindings.

use std::collections::{BTreeMap, HashSet, VecDeque};
use std::fmt;

use wit_parser::{
    Function, FunctionKind, InterfaceId, PackageId, Span, Type, TypeDefKind, TypeId, TypeOwner,
    World, WorldId, WorldItem, WorldKey,
};

use crate::Contract;
use crate::diagnostic::{Diagnostic, LocatedDiagnostic, Position, codes};
use crate::emit::ident;

/// Diagnostic code for two names in one scope that collide (ASCII case-insensitive or generated
/// identifier collision).
const CODE_IDENTIFIER_COLLISION: &str = codes::IDENTIFIER_COLLISION;

/// One construct outside the subset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    /// Where the construct appears, for example `alint:plugin/types/finding.span`.
    pub location: String,
    /// Why it is refused, with the supported alternative.
    pub message: String,
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.location, self.message)
    }
}

/// Collector that resolves `wit-parser` spans to physical file and 1-based UTF-16 source
/// positions while emitting located subset diagnostics.
struct Sink<'a> {
    contract: &'a Contract,
    cache: BTreeMap<String, crate::LineIndex>,
    out: Vec<LocatedDiagnostic>,
}

impl<'a> Sink<'a> {
    fn new(contract: &'a Contract) -> Self {
        Self {
            contract,
            cache: BTreeMap::new(),
            out: Vec::new(),
        }
    }

    fn resolve_span(&mut self, span: Span) -> Option<(String, Position)> {
        crate::resolve_contract_span_cached(self.contract, span, &mut self.cache)
    }

    fn package_location(&mut self, package_id: PackageId) -> Option<(String, Position)> {
        crate::package_location(self.contract, package_id, &mut self.cache)
    }

    fn emit_at(
        &mut self,
        code: &'static str,
        logical: impl Into<String>,
        message: impl Into<String>,
        location: Option<(String, Position)>,
    ) {
        let logical = logical.into();
        let mut diagnostic = Diagnostic::error(code, message).with_path(logical.clone());
        if let Some((_, pos)) = location {
            diagnostic = diagnostic.with_position(pos);
        }
        let mut located = LocatedDiagnostic::from_diagnostic(diagnostic).with_logical(logical);
        if let Some((file, _)) = location {
            located = located.with_file(file);
        }
        self.out.push(located);
    }

    fn refuse(
        &mut self,
        code: &'static str,
        logical: impl Into<String>,
        message: impl Into<String>,
        span: Span,
    ) {
        let loc = self.resolve_span(span);
        self.emit_at(code, logical, message, loc);
    }

    fn refuse_with_reason(
        &mut self,
        code: &'static str,
        logical: impl Into<String>,
        message: impl Into<String>,
        reason: Option<&str>,
        span: Span,
    ) {
        let message = match reason {
            Some(reason) => format!("{} ({reason})", message.into()),
            None => message.into(),
        };
        self.refuse(code, logical, message, span);
    }

    fn refuse_collision(
        &mut self,
        logical: impl Into<String>,
        message: impl Into<String>,
        span: Span,
    ) {
        self.refuse(CODE_IDENTIFIER_COLLISION, logical, message, span);
    }
}

/// Prefer `inner` when known, otherwise fall back to `outer`.
fn pick_span(inner: Span, outer: Span) -> Span {
    if inner.is_known() { inner } else { outer }
}

/// Check the root package and its standard `deps/` dependencies against the subset.
#[must_use]
pub fn check(contract: &Contract) -> Vec<Violation> {
    check_diagnostics(contract)
        .into_iter()
        .map(|diagnostic| Violation {
            location: diagnostic.path.unwrap_or_default(),
            message: diagnostic.message,
        })
        .collect()
}

/// Check the root package and its standard `deps/` dependencies and return structured
/// diagnostics with their stable per-rule subset codes (`WF1010`–`WF1030`, `WF1001` for
/// unrecognised constructs, and `WF1003` for identifier collisions).
#[must_use]
pub fn check_diagnostics(contract: &Contract) -> Vec<Diagnostic> {
    check_located_diagnostics(contract)
        .into_iter()
        .map(|located| located.diagnostic)
        .collect()
}

/// Check the root package and its standard `deps/` dependencies and return located diagnostics
/// carrying both the declaring physical source file/position and the logical contract path.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn check_located_diagnostics(contract: &Contract) -> Vec<LocatedDiagnostic> {
    let resolve = &contract.resolve;
    let package = &resolve.packages[contract.package];
    let pkg = format!("{}:{}", package.name.namespace, package.name.name);
    let mut sink = Sink::new(contract);
    let mut required_dependency_types = HashSet::new();

    if package.name.version.is_none() {
        let loc = sink.package_location(contract.package);
        sink.emit_at(
            codes::SUBSET_PACKAGE_VERSION,
            &pkg,
            "the contract package needs a version (`package ns:name@x.y.0;`) so `@since` gates \
             and evolution checks are possible",
            loc,
        );
    } else if let Some(version) = &package.name.version
        && (version.patch != 0
            || !version.pre.as_str().is_empty()
            || !version.build.as_str().is_empty())
    {
        let loc = sink.package_location(contract.package);
        sink.emit_at(
            codes::SUBSET_PACKAGE_VERSION,
            &pkg,
            "the contract version must be `<major>.<minor>.0`; patch, pre-release, and \
             build metadata are not supported",
            loc,
        );
    }

    // A contract is one package. Any other package in the resolve comes from a nested
    // `package ns:name { ... }` block or a dependency, and nothing in it is locked or generated,
    // so it is refused rather than silently left out of the output.
    for (id, other) in &resolve.packages {
        if id != contract.package && !contract.is_dependency(id) {
            let loc = sink
                .package_location(id)
                .or_else(|| sink.package_location(contract.package));
            sink.emit_at(
                codes::SUBSET_NESTED_PACKAGE,
                &pkg,
                format!(
                    "the version directory also loads package `{}` from a nested `package` \
                     block; dependencies must live under the standard `deps/` directory",
                    other.name
                ),
                loc,
            );
        }
    }

    // Every package under `deps/` contributes to the generated dependency surface, whether or
    // not the root references it, so every dependency package's version and interface scope are
    // checked here as subset diagnostics (exit 1).
    let mut dependencies = contract.dependencies.clone();
    dependencies.sort_by(|left, right| {
        let left_name = &resolve.packages[*left].name;
        let right_name = &resolve.packages[*right].name;
        (&left_name.namespace, &left_name.name).cmp(&(&right_name.namespace, &right_name.name))
    });
    let mut seen_packages: BTreeMap<String, (&str, String)> = BTreeMap::new();
    let mut seen_dep_rust_mods: BTreeMap<String, String> = BTreeMap::new();
    let mut seen_dep_proto_files: BTreeMap<String, String> = BTreeMap::new();
    seen_packages.insert(pkg.to_ascii_lowercase(), ("root package", pkg.clone()));
    for dep_id in dependencies {
        let dep = &resolve.packages[dep_id];
        let dep_pkg = format!("{}:{}", dep.name.namespace, dep.name.name);
        let dep_loc = sink.package_location(dep_id);
        let folded_pkg = dep_pkg.to_ascii_lowercase();
        if let Some((prev_kind, prev_name)) = seen_packages.get(&folded_pkg) {
            if prev_name != &dep_pkg {
                sink.emit_at(
                    CODE_IDENTIFIER_COLLISION,
                    &dep_pkg,
                    format!(
                        "dependency package `{dep_pkg}` differs only in letter case from \
                         {prev_kind} `{prev_name}`; give them distinct names"
                    ),
                    dep_loc.clone(),
                );
            }
        } else {
            seen_packages.insert(folded_pkg, ("dependency package", dep_pkg.clone()));
        }
        if let Ok(dep_mod) = ident::plain_ident(
            &format!(
                "{}_{}",
                ident::plain_snake(&dep.name.namespace),
                ident::plain_snake(&dep.name.name)
            ),
            "dependency module",
        ) && let Some(prev) = seen_dep_rust_mods.insert(dep_mod.clone(), dep_pkg.clone())
            && !prev.eq_ignore_ascii_case(&dep_pkg)
        {
            sink.emit_at(
                CODE_IDENTIFIER_COLLISION,
                &dep_pkg,
                format!(
                    "dependency packages `{prev}` and `{dep_pkg}` both map to generated module `{dep_mod}`"
                ),
                dep_loc.clone(),
            );
        }
        if let Some(version) = &dep.name.version {
            let proto_file = format!(
                "{}.{}.v{}.proto",
                ident::proto_package_segment(&dep.name.namespace),
                ident::proto_package_segment(&dep.name.name),
                version.major
            );
            if let Some(prev) = seen_dep_proto_files.insert(proto_file.clone(), dep_pkg.clone())
                && !prev.eq_ignore_ascii_case(&dep_pkg)
            {
                sink.emit_at(
                    CODE_IDENTIFIER_COLLISION,
                    &dep_pkg,
                    format!(
                        "dependency packages `{prev}` and `{dep_pkg}` both generate `{proto_file}`"
                    ),
                    dep_loc.clone(),
                );
            }
        }
        let entry = contract
            .dependency_entry(dep_id)
            .map_or_else(|| "deps/".to_string(), |path| path.display().to_string());
        match &dep.name.version {
            None => sink.emit_at(
                codes::SUBSET_DEPENDENCY_VERSION,
                &dep_pkg,
                format!(
                    "dependency package `{dep_pkg}` in `{entry}` has no version; dependency \
                     versions must be `<major>.<minor>.0`"
                ),
                dep_loc,
            ),
            Some(version)
                if version.patch != 0
                    || !version.pre.as_str().is_empty()
                    || !version.build.as_str().is_empty() =>
            {
                sink.emit_at(
                    codes::SUBSET_DEPENDENCY_VERSION,
                    &dep_pkg,
                    format!(
                        "dependency package `{dep_pkg}@{version}` in `{entry}` has unsupported \
                         version `{version}`; dependency versions must be `<major>.<minor>.0` \
                         (patch, pre-release, and build metadata are not supported)"
                    ),
                    dep_loc,
                );
            }
            Some(_) => {}
        }
        check_scope_case_collisions(
            &dep_pkg,
            "interface",
            dep.interfaces
                .iter()
                .map(|(name, id)| (name.as_str(), resolve.interfaces[*id].span)),
            |name| format!("{dep_pkg}/{name}"),
            &mut sink,
        );
    }

    // Package scope: interfaces and worlds share one WIT package namespace; names differing only
    // in ASCII letter case map to one generated identifier and are refused.
    check_package_case_collisions(resolve, &pkg, package, &mut sink);
    check_package_generated_collisions(resolve, &pkg, package, &mut sink);

    for (iface_name, &iface_id) in &package.interfaces {
        let iface = &resolve.interfaces[iface_id];
        check_wit_stability(
            &iface.stability,
            &format!("{pkg}/{iface_name}"),
            None,
            iface.span,
            &mut sink,
        );
        check_interface_case_collisions(resolve, &pkg, iface_name, iface, &mut sink);
        // `use` aliases are owned by the interface that names them, so they are checked here
        // too: an alias of a type from another package is refused at the `use`.
        for (type_name, &type_id) in &iface.types {
            let at = format!("{pkg}/{iface_name}/{type_name}");
            let type_span = pick_span(resolve.types[type_id].span, iface.span);
            check_wit_stability(
                &resolve.types[type_id].stability,
                &at,
                None,
                type_span,
                &mut sink,
            );
            check_external_id(
                resolve.types[type_id].external_id.as_deref(),
                &at,
                None,
                type_span,
                &mut sink,
            );
            check_def(contract, type_id, &at, None, type_span, &mut sink);
            check_dependency_def(
                contract,
                type_id,
                &at,
                &mut required_dependency_types,
                None,
                &mut sink,
            );
        }
        for (func_name, func) in &iface.functions {
            let at = format!("{pkg}/{iface_name}#{func_name}");
            let func_span = pick_span(func.span, iface.span);
            check_wit_stability(&func.stability, &at, None, func_span, &mut sink);
            check_external_id(func.external_id.as_deref(), &at, None, func_span, &mut sink);
            check_function(contract, func, &at, None, func_span, &mut sink);
            check_dependency_function_refs(
                contract,
                func,
                &at,
                &mut required_dependency_types,
                None,
                &mut sink,
            );
        }
    }

    for (world_name, &world_id) in &package.worlds {
        check_world(
            contract,
            world_id,
            &format!("{pkg}/{world_name}"),
            &mut required_dependency_types,
            &mut sink,
        );
    }
    if sink.out.is_empty() {
        check_model_and_emitters(contract, &pkg, &mut sink);
    }
    sink.out
}

#[allow(clippy::too_many_lines)]
fn check_model_and_emitters(contract: &Contract, pkg: &str, sink: &mut Sink<'_>) {
    let mut locations: Option<BTreeMap<String, (String, Position)>> = None;
    let pkg_loc = sink.package_location(contract.package);
    let model = match crate::model::build_unchecked(contract) {
        Ok(model) => model,
        Err(err) => {
            if let Some(diags) = crate::error_located_diagnostics(&err) {
                sink.out.extend(diags.iter().cloned());
            } else {
                sink.emit_at(codes::SUBSET_UNSUPPORTED, pkg, format!("{err:#}"), pkg_loc);
            }
            return;
        }
    };
    let limit_errors = model.collect_type_limit_errors();
    if !limit_errors.is_empty() {
        let map = locations.get_or_insert_with(|| contract.member_locations());
        for (location, message) in limit_errors {
            let loc = map.get(&location).cloned().or_else(|| pkg_loc.clone());
            sink.emit_at(codes::SUBSET_TYPE_LIMIT, location, message, loc);
        }
        return;
    }
    check_dependency_model_wit_stability(contract, &model, sink);
    let stability_errors = stability_errors(&model);
    if !stability_errors.is_empty() {
        let map = locations.get_or_insert_with(|| contract.member_locations());
        for (location, message) in stability_errors {
            let loc = map.get(&location).cloned().or_else(|| pkg_loc.clone());
            sink.emit_at(codes::SUBSET_ANNOTATION, location, message, loc);
        }
    }
    if !sink.out.is_empty() {
        return;
    }
    let checks: [anyhow::Result<()>; 4] = [
        crate::emit::rust::validate_model(&model),
        crate::emit::proto::validate_model(&model),
        crate::emit::json_schema::validate_model(&model),
        crate::emit::glue::validate_model(&model),
    ];
    let mut seen_messages = HashSet::new();
    for result in checks {
        let Err(err) = result else {
            continue;
        };
        if let Some(diags) = crate::error_located_diagnostics(&err) {
            let map = locations.get_or_insert_with(|| contract.member_locations());
            for located in diags {
                if seen_messages.insert(located.diagnostic.message.clone()) {
                    let mut located = located.clone();
                    if located.file.is_none()
                        && let Some((file, pos)) = located
                            .logical
                            .as_deref()
                            .or(located.diagnostic.path.as_deref())
                            .and_then(|path| map.get(path).cloned())
                            .or_else(|| pkg_loc.clone())
                    {
                        if located.diagnostic.position.is_none() {
                            located.diagnostic.position = Some(pos);
                        }
                        located = located.with_file(file);
                    }
                    sink.out.push(located);
                }
            }
            continue;
        }
        if let Some(diags) = crate::error_diagnostics(&err) {
            let map = locations.get_or_insert_with(|| contract.member_locations());
            for diag in diags {
                if seen_messages.insert(diag.message.clone()) {
                    let mut located = LocatedDiagnostic::from_diagnostic(diag.clone());
                    if let Some(path) = &diag.path {
                        located = located.with_logical(path.clone());
                    }
                    if let Some((file, pos)) = diag
                        .path
                        .as_deref()
                        .and_then(|path| map.get(path).cloned())
                        .or_else(|| pkg_loc.clone())
                    {
                        if located.diagnostic.position.is_none() {
                            located.diagnostic.position = Some(pos);
                        }
                        located = located.with_file(file);
                    }
                    sink.out.push(located);
                }
            }
            continue;
        }
        let message = format!("{err:#}");
        if seen_messages.insert(message.clone()) {
            let code = if message.contains("-byte limit") {
                codes::SUBSET_TYPE_LIMIT
            } else if crate::is_contract_error(&err) {
                CODE_IDENTIFIER_COLLISION
            } else {
                codes::SUBSET_UNSUPPORTED
            };
            sink.emit_at(code, pkg, message, pkg_loc.clone());
        }
    }
}

fn check_dependency_model_wit_stability(
    contract: &Contract,
    model: &crate::model::Model,
    sink: &mut Sink<'_>,
) {
    let resolve = &contract.resolve;
    for dependency in &model.dependencies {
        let Some(&dep_id) = contract.dependencies.iter().find(|&&id| {
            let pkg = &resolve.packages[id].name;
            pkg.namespace == dependency.package.namespace && pkg.name == dependency.package.name
        }) else {
            continue;
        };
        let dep = &resolve.packages[dep_id];
        let dep_pkg = format!("{}:{}", dep.name.namespace, dep.name.name);
        for interface in &dependency.interfaces {
            let Some(&iface_id) = dep.interfaces.get(&interface.name) else {
                continue;
            };
            let iface = &resolve.interfaces[iface_id];
            let iface_path = format!("{dep_pkg}/{}", interface.name);
            check_wit_stability(&iface.stability, &iface_path, None, iface.span, sink);
            for def in &interface.types {
                if let Some(&type_id) = iface.types.get(&def.name) {
                    let wit_def = &resolve.types[type_id];
                    check_wit_stability(
                        &wit_def.stability,
                        &format!("{iface_path}/{}", def.name),
                        None,
                        pick_span(wit_def.span, iface.span),
                        sink,
                    );
                }
            }
            for func in &interface.functions {
                if let Some(wit_func) = iface.functions.get(&func.name) {
                    check_wit_stability(
                        &wit_func.stability,
                        &format!("{iface_path}#{}", func.name),
                        None,
                        pick_span(wit_func.span, iface.span),
                        sink,
                    );
                }
            }
        }
    }
}

fn stability_errors(model: &crate::model::Model) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let package = format!("{}:{}", model.package.namespace, model.package.name);
    collect_interface_stability_errors(
        &package,
        model.package.version,
        &model.interfaces,
        &mut out,
    );
    for world in &model.worlds {
        let world_path = format!("{package}/{}", world.name);
        collect_stability_errors(
            &world_path,
            model.package.version,
            &world.stability,
            &mut out,
        );
        for (side, interfaces) in [("import", &world.imports), ("export", &world.exports)] {
            for interface in interfaces {
                collect_stability_errors(
                    &format!("{world_path}/{side}/{}", interface.qualified_name()),
                    model.package.version,
                    &interface.stability,
                    &mut out,
                );
            }
        }
    }
    for dependency in &model.dependencies {
        let package = format!(
            "{}:{}",
            dependency.package.namespace, dependency.package.name
        );
        collect_interface_stability_errors(
            &package,
            dependency.package.version,
            &dependency.interfaces,
            &mut out,
        );
    }
    out
}

fn collect_interface_stability_errors(
    package: &str,
    package_version: crate::model::Version,
    interfaces: &[crate::model::Interface],
    out: &mut Vec<(String, String)>,
) {
    for interface in interfaces {
        let interface_path = format!("{package}/{}", interface.name);
        collect_stability_errors(&interface_path, package_version, &interface.stability, out);
        for def in &interface.types {
            collect_stability_errors(
                &format!("{interface_path}/{}", def.name),
                package_version,
                &def.stability,
                out,
            );
        }
        for function in &interface.functions {
            collect_stability_errors(
                &format!("{interface_path}#{}", function.name),
                package_version,
                &function.stability,
                out,
            );
        }
    }
}

fn collect_stability_errors(
    path: &str,
    package_version: crate::model::Version,
    stability: &crate::model::Stability,
    out: &mut Vec<(String, String)>,
) {
    for (annotation, version) in [
        ("@since", stability.since()),
        ("@deprecated", stability.deprecated()),
    ] {
        let Some(version) = version else {
            continue;
        };
        if version.patch != 0 {
            out.push((
                path.to_string(),
                format!(
                    "{annotation} version `{version}` must be `<major>.<minor>.0`; patch versions are not supported"
                ),
            ));
        }
        if version > package_version {
            out.push((
                path.to_string(),
                format!(
                    "{annotation} version `{version}` is later than containing package version `{package_version}`"
                ),
            ));
        }
    }
    if let (Some(since), Some(deprecated)) = (stability.since(), stability.deprecated())
        && deprecated < since
    {
        out.push((
            path.to_string(),
            format!("@deprecated version `{deprecated}` is earlier than @since version `{since}`"),
        ));
    }
}

fn check_package_case_collisions(
    resolve: &wit_parser::Resolve,
    pkg: &str,
    package: &wit_parser::Package,
    sink: &mut Sink<'_>,
) {
    let mut seen: BTreeMap<String, (&str, &str)> = BTreeMap::new();
    let items = package
        .interfaces
        .iter()
        .map(|(name, id)| ("interface", name.as_str(), resolve.interfaces[*id].span))
        .chain(
            package
                .worlds
                .iter()
                .map(|(name, id)| ("world", name.as_str(), resolve.worlds[*id].span)),
        );
    for (kind, name, span) in items {
        let folded = name.to_ascii_lowercase();
        if let Some((prev_kind, prev_name)) = seen.get(&folded) {
            if *prev_name != name {
                sink.refuse_collision(
                    format!("{pkg}/{name}"),
                    format!(
                        "{kind} `{name}` differs only in letter case from {prev_kind} \
                         `{prev_name}` in `{pkg}`; give them distinct names"
                    ),
                    span,
                );
            }
        } else {
            seen.insert(folded, (kind, name));
        }
    }
}

fn check_interface_case_collisions(
    resolve: &wit_parser::Resolve,
    pkg: &str,
    iface_name: &str,
    iface: &wit_parser::Interface,
    sink: &mut Sink<'_>,
) {
    let scope = format!("{pkg}/{iface_name}");
    let mut seen: BTreeMap<String, (&str, &str)> = BTreeMap::new();
    let items = iface
        .types
        .iter()
        .map(|(name, id)| {
            (
                "type",
                name.as_str(),
                pick_span(resolve.types[*id].span, iface.span),
            )
        })
        .chain(
            iface
                .functions
                .iter()
                .map(|(name, func)| ("function", name.as_str(), pick_span(func.span, iface.span))),
        );
    for (kind, name, span) in items {
        let folded = name.to_ascii_lowercase();
        if let Some((prev_kind, prev_name)) = seen.get(&folded) {
            if *prev_name != name {
                let location = if kind == "function" {
                    format!("{scope}#{name}")
                } else {
                    format!("{scope}/{name}")
                };
                sink.refuse_collision(
                    location,
                    format!(
                        "{kind} `{name}` differs only in letter case from {prev_kind} \
                         `{prev_name}` in `{scope}`; give them distinct names"
                    ),
                    span,
                );
            }
        } else {
            seen.insert(folded, (kind, name));
        }
    }
}

fn check_scope_case_collisions<'a>(
    scope: &str,
    kind: &str,
    names: impl Iterator<Item = (&'a str, Span)>,
    location: impl Fn(&str) -> String,
    sink: &mut Sink<'_>,
) {
    let mut seen: BTreeMap<String, &str> = BTreeMap::new();
    for (name, span) in names {
        let folded = name.to_ascii_lowercase();
        if let Some(&prev) = seen.get(&folded) {
            if prev != name {
                sink.refuse_collision(
                    location(name),
                    format!(
                        "{kind} `{name}` differs only in letter case from {kind} `{prev}` in \
                         `{scope}`; give them distinct names"
                    ),
                    span,
                );
            }
        } else {
            seen.insert(folded, name);
        }
    }
}

fn check_wit_stability(
    stability: &wit_parser::Stability,
    at: &str,
    reason: Option<&str>,
    span: Span,
    sink: &mut Sink<'_>,
) {
    let versions = match stability {
        wit_parser::Stability::Unknown => [("@since", None), ("@deprecated", None)],
        wit_parser::Stability::Unstable { deprecated, .. } => {
            [("@since", None), ("@deprecated", deprecated.as_ref())]
        }
        wit_parser::Stability::Stable { since, deprecated } => [
            ("@since", Some(since)),
            ("@deprecated", deprecated.as_ref()),
        ],
    };
    for (annotation, version) in versions {
        let Some(version) = version else {
            continue;
        };
        if !version.pre.as_str().is_empty() || !version.build.as_str().is_empty() {
            sink.refuse_with_reason(
                codes::SUBSET_ANNOTATION,
                at,
                format!(
                    "{annotation} version `{version}` must be `<major>.<minor>.0`; \
                     patch, pre-release, and build metadata are not supported"
                ),
                reason,
                span,
            );
        }
    }
}

fn check_external_id(
    external_id: Option<&str>,
    at: &str,
    reason: Option<&str>,
    span: Span,
    sink: &mut Sink<'_>,
) {
    if let Some(id) = external_id {
        sink.refuse_with_reason(
            codes::SUBSET_ANNOTATION,
            at,
            format!(
                "annotation `@external-id(\"{id}\")` is not supported; only `@since`, \
                 `@unstable`, and `@deprecated` are accepted"
            ),
            reason,
            span,
        );
    }
}

#[allow(clippy::too_many_lines)]
fn check_world(
    contract: &Contract,
    world_id: WorldId,
    at: &str,
    required_dependency_types: &mut HashSet<TypeId>,
    sink: &mut Sink<'_>,
) {
    let resolve = &contract.resolve;
    let world = &resolve.worlds[world_id];
    check_wit_stability(&world.stability, at, None, world.span, sink);
    for (verb, singular, items) in [
        ("imports", "import", &world.imports),
        ("exports", "export", &world.exports),
    ] {
        for (key, item) in items {
            let item_span = pick_span(item.span(), world.span);
            match (key, item) {
                (
                    WorldKey::Name(name),
                    WorldItem::Interface {
                        id, external_id, ..
                    },
                ) => {
                    let item_at = format!("{at}/{name}");
                    check_external_id(external_id.as_deref(), &item_at, None, item_span, sink);
                    let iface = &resolve.interfaces[*id];
                    if let Some(iface_name) = iface.name.as_deref() {
                        let target = if iface.package == Some(contract.package) {
                            iface_name.to_string()
                        } else {
                            resolve.id_of(*id).unwrap_or_else(|| iface_name.to_string())
                        };
                        sink.refuse(
                            codes::SUBSET_NAMED_WORLD_INTERFACE,
                            item_at,
                            format!(
                                "named interface {singular}s are not supported \
                                 (`{singular} {name}: {target}`); {singular} `{target}` directly \
                                 without a local name"
                            ),
                            item_span,
                        );
                    } else {
                        sink.refuse(
                            codes::SUBSET_INLINE_INTERFACE,
                            item_at,
                            "inline interfaces are not supported; define a named interface in the \
                             package and import or export it",
                            pick_span(iface.span, item_span),
                        );
                    }
                }
                (
                    WorldKey::Interface(_),
                    WorldItem::Interface {
                        id,
                        stability,
                        external_id,
                        ..
                    },
                ) => {
                    let is_implicit = singular == "import"
                        && !contract.is_explicit_world_import(&world.name, *id);
                    if !is_implicit {
                        let label = interface_display_name(contract, *id);
                        check_wit_stability(
                            stability,
                            &format!("{at}/{singular}/{label}"),
                            None,
                            item_span,
                            sink,
                        );
                        check_external_id(
                            external_id.as_deref(),
                            &format!("{at}/{label}"),
                            None,
                            item_span,
                            sink,
                        );
                    }
                    let iface = &resolve.interfaces[*id];
                    if let Some(package) = iface.package {
                        if package != contract.package {
                            if contract.is_dependency(package) {
                                let reason = if is_implicit {
                                    implicit_import_reason(contract, world, *id)
                                } else {
                                    let label = interface_display_name(contract, *id);
                                    format!(
                                        "required by world `{}` {singular} `{label}`",
                                        world.name
                                    )
                                };
                                check_dependency_interface(
                                    contract,
                                    *id,
                                    required_dependency_types,
                                    Some(&reason),
                                    sink,
                                );
                            } else {
                                // `id_of` panics on an interface without a package.
                                let name = resolve.id_of(*id).unwrap_or_default();
                                sink.refuse(
                                    codes::SUBSET_WORLD_FOREIGN_INTERFACE,
                                    at,
                                    format!(
                                        "{verb} `{name}` from a package outside the standard \
                                         `deps/` directory"
                                    ),
                                    item_span,
                                );
                            }
                        }
                    } else {
                        sink.refuse(
                            codes::SUBSET_WORLD_FOREIGN_INTERFACE,
                            at,
                            format!("{verb} an interface with no package"),
                            item_span,
                        );
                    }
                }
                (_, WorldItem::Function(func)) => {
                    let item_at = format!("{at}#{}", func.name);
                    let func_span = pick_span(func.span, item_span);
                    check_external_id(func.external_id.as_deref(), &item_at, None, func_span, sink);
                    sink.refuse(
                        codes::SUBSET_WORLD_FUNCTION,
                        item_at,
                        "functions must be defined in an interface; export the interface from the \
                         world",
                        func_span,
                    );
                }
                (_, WorldItem::Type { id, .. }) => {
                    let type_def = &resolve.types[*id];
                    let type_name = type_def.name.as_deref().unwrap_or_default();
                    let type_span = pick_span(type_def.span, item_span);
                    let item_at = format!("{at}/{type_name}");
                    check_external_id(
                        type_def.external_id.as_deref(),
                        &item_at,
                        None,
                        type_span,
                        sink,
                    );
                    if type_def.owner == TypeOwner::World(world_id) {
                        check_wit_stability(&type_def.stability, &item_at, None, type_span, sink);
                        if let Some(pkg_ver) =
                            resolve.packages[contract.package].name.version.as_ref()
                        {
                            let pkg_ver = crate::model::Version::new(
                                pkg_ver.major,
                                pkg_ver.minor,
                                pkg_ver.patch,
                            );
                            let stability = crate::model::stability(&type_def.stability);
                            let mut errors = Vec::new();
                            collect_stability_errors(&item_at, pkg_ver, &stability, &mut errors);
                            for (location, message) in errors {
                                sink.refuse(codes::SUBSET_ANNOTATION, location, message, type_span);
                            }
                        }
                    }
                    check_world_type(contract, world_id, *id, at, type_span, sink);
                    check_dependency_def(contract, *id, at, required_dependency_types, None, sink);
                }
            }
        }
    }
}

fn interface_display_name(contract: &Contract, id: InterfaceId) -> String {
    let resolve = &contract.resolve;
    let iface = &resolve.interfaces[id];
    let name = iface.name.as_deref().unwrap_or_default();
    match iface.package {
        Some(pkg) if pkg != contract.package => {
            let package = &resolve.packages[pkg].name;
            format!("{}:{}/{name}", package.namespace, package.name)
        }
        _ => name.to_string(),
    }
}

/// Explain which world item caused `wit-parser` to implicitly import dependency `target`.
fn implicit_import_reason(contract: &Contract, world: &World, target: InterfaceId) -> String {
    let resolve = &contract.resolve;
    for (key, item) in &world.exports {
        if let WorldItem::Interface { id, .. } = item
            && interface_reaches(resolve, *id, target)
        {
            let label = world_interface_label(contract, key, *id);
            return format!(
                "implicitly imported by world `{}` via exported interface `{label}`",
                world.name
            );
        }
    }
    for (key, item) in &world.imports {
        let WorldItem::Interface { id, .. } = item else {
            continue;
        };
        if *id == target {
            continue;
        }
        let is_explicit = match key {
            WorldKey::Name(_) => true,
            WorldKey::Interface(_) => contract.is_explicit_world_import(&world.name, *id),
        };
        if is_explicit && interface_reaches(resolve, *id, target) {
            let label = world_interface_label(contract, key, *id);
            return format!(
                "implicitly imported by world `{}` via imported interface `{label}`",
                world.name
            );
        }
    }
    for (key, item) in &world.imports {
        if let WorldItem::Type { id, .. } = item
            && type_reaches_interface(resolve, *id, target)
        {
            let type_name = match key {
                WorldKey::Name(name) => name.as_str(),
                WorldKey::Interface(_) => resolve.types[*id].name.as_deref().unwrap_or_default(),
            };
            return format!(
                "implicitly imported by world `{}` via world type `{type_name}`",
                world.name
            );
        }
    }
    for (ns, pkg_name, foreign_world_name) in contract.included_foreign_worlds(&world.name) {
        if foreign_world_reaches_interface(resolve, ns, pkg_name, foreign_world_name, target) {
            return format!(
                "implicitly imported by world `{}` via included world \
                 `{ns}:{pkg_name}/{foreign_world_name}`",
                world.name
            );
        }
    }
    format!("implicitly imported by world `{}`", world.name)
}

fn world_interface_label(contract: &Contract, key: &WorldKey, id: InterfaceId) -> String {
    match key {
        WorldKey::Name(name) => name.clone(),
        WorldKey::Interface(_) => interface_display_name(contract, id),
    }
}

fn foreign_world_reaches_interface(
    resolve: &wit_parser::Resolve,
    namespace: &str,
    package_name: &str,
    world_name: &str,
    target: InterfaceId,
) -> bool {
    let Some((_, package)) = resolve
        .packages
        .iter()
        .find(|(_, pkg)| pkg.name.namespace == namespace && pkg.name.name == package_name)
    else {
        return false;
    };
    let Some(&world_id) = package.worlds.get(world_name) else {
        return false;
    };
    let world = &resolve.worlds[world_id];
    world
        .imports
        .values()
        .chain(world.exports.values())
        .any(|item| match item {
            WorldItem::Interface { id, .. } => {
                *id == target || interface_reaches(resolve, *id, target)
            }
            WorldItem::Type { id, .. } => type_reaches_interface(resolve, *id, target),
            WorldItem::Function(_) => false,
        })
}

fn interface_reaches(
    resolve: &wit_parser::Resolve,
    start: InterfaceId,
    target: InterfaceId,
) -> bool {
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();
    for dep in resolve.interface_direct_deps(start) {
        if visited.insert(dep) {
            queue.push_back(dep);
        }
    }
    while let Some(current) = queue.pop_front() {
        if current == target {
            return true;
        }
        for dep in resolve.interface_direct_deps(current) {
            if visited.insert(dep) {
                queue.push_back(dep);
            }
        }
    }
    false
}

fn type_reaches_interface(
    resolve: &wit_parser::Resolve,
    type_id: TypeId,
    target: InterfaceId,
) -> bool {
    let mut visited = HashSet::new();
    let mut current = type_id;
    while visited.insert(current) {
        let def = &resolve.types[current];
        if let TypeOwner::Interface(iface_id) = def.owner {
            return iface_id == target || interface_reaches(resolve, iface_id, target);
        }
        if let TypeDefKind::Type(Type::Id(next)) = def.kind {
            current = next;
        } else {
            break;
        }
    }
    false
}

/// Validate every item exposed by a dependency interface and every named type it reaches.
///
/// Unsupported items in an otherwise unused dependency remain ignorable. Importing or exporting
/// the interface from a root world makes its complete API part of the root contract, so dropping
/// one of those items would silently generate a different interface.
fn check_dependency_interface(
    contract: &Contract,
    interface_id: InterfaceId,
    visited: &mut HashSet<TypeId>,
    reason: Option<&str>,
    sink: &mut Sink<'_>,
) {
    let resolve = &contract.resolve;
    let interface = &resolve.interfaces[interface_id];
    let Some(package_id) = interface.package else {
        return;
    };
    let package = &resolve.packages[package_id];
    let dep_pkg = format!("{}:{}", package.name.namespace, package.name.name);
    let interface_name = interface.name.as_deref().unwrap_or_default();
    check_interface_case_collisions(resolve, &dep_pkg, interface_name, interface, sink);
    let base = format!("{dep_pkg}/{interface_name}");
    check_wit_stability(&interface.stability, &base, reason, interface.span, sink);
    for (name, id) in &interface.types {
        check_dependency_def(
            contract,
            *id,
            &format!("{base}/{name}"),
            visited,
            reason,
            sink,
        );
    }
    for (name, function) in &interface.functions {
        let at = format!("{base}#{name}");
        let func_span = pick_span(function.span, interface.span);
        check_wit_stability(&function.stability, &at, reason, func_span, sink);
        check_external_id(
            function.external_id.as_deref(),
            &at,
            reason,
            func_span,
            sink,
        );
        check_function(contract, function, &at, reason, func_span, sink);
        check_dependency_function_refs(contract, function, &at, visited, reason, sink);
    }
}

fn check_dependency_function_refs(
    contract: &Contract,
    function: &Function,
    at: &str,
    visited: &mut HashSet<TypeId>,
    reason: Option<&str>,
    sink: &mut Sink<'_>,
) {
    for param in &function.params {
        check_dependency_type_refs(
            contract,
            &param.ty,
            &format!("{at}({})", param.name),
            visited,
            reason,
            sink,
        );
    }
    if let Some(result) = &function.result {
        check_dependency_type_refs(
            contract,
            result,
            &format!("{at} -> result"),
            visited,
            reason,
            sink,
        );
    }
}

fn check_dependency_def(
    contract: &Contract,
    id: TypeId,
    at: &str,
    visited: &mut HashSet<TypeId>,
    reason: Option<&str>,
    sink: &mut Sink<'_>,
) {
    if !visited.insert(id) {
        return;
    }
    if is_dependency_def(contract, id) {
        let def = &contract.resolve.types[id];
        let def_span = def.span;
        check_wit_stability(&def.stability, at, reason, def_span, sink);
        check_external_id(def.external_id.as_deref(), at, reason, def_span, sink);
        check_def(contract, id, at, reason, def_span, sink);
    }
    check_dependency_def_refs(contract, id, at, visited, reason, sink);
}

fn check_dependency_def_refs(
    contract: &Contract,
    id: TypeId,
    at: &str,
    visited: &mut HashSet<TypeId>,
    reason: Option<&str>,
    sink: &mut Sink<'_>,
) {
    match &contract.resolve.types[id].kind {
        TypeDefKind::Record(record) => {
            for field in &record.fields {
                check_dependency_type_refs(
                    contract,
                    &field.ty,
                    &format!("{at}.{}", field.name),
                    visited,
                    reason,
                    sink,
                );
            }
        }
        TypeDefKind::Variant(variant) => {
            for case in &variant.cases {
                if let Some(ty) = &case.ty {
                    check_dependency_type_refs(
                        contract,
                        ty,
                        &format!("{at}.{}", case.name),
                        visited,
                        reason,
                        sink,
                    );
                }
            }
        }
        TypeDefKind::Option(ty)
        | TypeDefKind::List(ty)
        | TypeDefKind::FixedLengthList(ty, _)
        | TypeDefKind::Type(ty) => {
            check_dependency_type_refs(contract, ty, at, visited, reason, sink);
        }
        TypeDefKind::Map(key, value) => {
            check_dependency_type_refs(contract, key, at, visited, reason, sink);
            check_dependency_type_refs(contract, value, at, visited, reason, sink);
        }
        TypeDefKind::Result(result) => {
            for ty in [&result.ok, &result.err].into_iter().flatten() {
                check_dependency_type_refs(contract, ty, at, visited, reason, sink);
            }
        }
        TypeDefKind::Enum(_)
        | TypeDefKind::Flags(_)
        | TypeDefKind::Tuple(_)
        | TypeDefKind::Resource
        | TypeDefKind::Handle(_)
        | TypeDefKind::Future(_)
        | TypeDefKind::Stream(_)
        | TypeDefKind::Unknown => {}
    }
}

fn check_dependency_type_refs(
    contract: &Contract,
    ty: &Type,
    at: &str,
    visited: &mut HashSet<TypeId>,
    reason: Option<&str>,
    sink: &mut Sink<'_>,
) {
    let Type::Id(id) = ty else {
        return;
    };
    let def = &contract.resolve.types[*id];
    if def.name.is_some() {
        let location = is_dependency_def(contract, *id)
            .then(|| dependency_type_location(contract, *id))
            .flatten();
        check_dependency_def(
            contract,
            *id,
            location.as_deref().unwrap_or(at),
            visited,
            reason,
            sink,
        );
    } else {
        check_dependency_def_refs(contract, *id, at, visited, reason, sink);
    }
}

fn is_dependency_def(contract: &Contract, id: TypeId) -> bool {
    let TypeOwner::Interface(interface_id) = contract.resolve.types[id].owner else {
        return false;
    };
    contract.resolve.interfaces[interface_id]
        .package
        .is_some_and(|package| contract.is_dependency(package))
}

fn dependency_type_location(contract: &Contract, id: TypeId) -> Option<String> {
    let resolve = &contract.resolve;
    let def = &resolve.types[id];
    let name = def.name.as_deref()?;
    let TypeOwner::Interface(interface_id) = def.owner else {
        return None;
    };
    let interface = &resolve.interfaces[interface_id];
    let package = &resolve.packages[interface.package?];
    Some(format!(
        "{}:{}/{}/{}",
        package.name.namespace,
        package.name.name,
        interface.name.as_deref()?,
        name
    ))
}

fn check_world_type(
    contract: &Contract,
    world_id: WorldId,
    id: TypeId,
    at: &str,
    span: Span,
    sink: &mut Sink<'_>,
) {
    let resolve = &contract.resolve;
    let def = &resolve.types[id];
    let at = format!("{at}/{}", def.name.as_deref().unwrap_or_default());
    let type_span = pick_span(def.span, span);
    match &def.kind {
        // Brought in by `include`: checked where it is defined, or refused if that is in
        // another package.
        _ if def.owner != TypeOwner::World(world_id) => {
            check_type(contract, &Type::Id(id), &at, None, type_span, sink);
        }
        // `use` in a world names a type defined in an interface; it defines nothing itself.
        TypeDefKind::Type(target @ Type::Id(target_id))
            if resolve.types[*target_id].name.is_some() =>
        {
            check_type(contract, target, &at, None, type_span, sink);
        }
        _ => sink.refuse(
            codes::SUBSET_WORLD_TYPE,
            at,
            "types must be defined in an interface, not directly in a world",
            type_span,
        ),
    }
}

fn check_function(
    contract: &Contract,
    func: &Function,
    at: &str,
    reason: Option<&str>,
    span: Span,
    sink: &mut Sink<'_>,
) {
    let func_span = pick_span(func.span, span);
    match func.kind {
        FunctionKind::Freestanding => {
            check_scope_case_collisions(
                at,
                "parameter",
                func.params
                    .iter()
                    .map(|param| (param.name.as_str(), pick_span(param.span, func_span))),
                |name| format!("{at}({name})"),
                sink,
            );
            for param in &func.params {
                let param_span = pick_span(param.span, func_span);
                check_type(
                    contract,
                    &param.ty,
                    &format!("{at}({})", param.name),
                    reason,
                    param_span,
                    sink,
                );
            }
            if let Some(result) = &func.result {
                check_type(
                    contract,
                    result,
                    &format!("{at} -> result"),
                    reason,
                    func_span,
                    sink,
                );
            }
        }
        FunctionKind::AsyncFreestanding => sink.refuse_with_reason(
            codes::SUBSET_ASYNC_FUNCTION,
            at,
            "`async` functions are not supported; the contract is synchronous",
            reason,
            func_span,
        ),
        FunctionKind::Getter | FunctionKind::Setter => sink.refuse_with_reason(
            codes::SUBSET_ACCESSOR,
            at,
            "`get` and `set` accessors are not supported; use a function",
            reason,
            func_span,
        ),
        // A resource's functions: the resource itself is refused where it is defined.
        FunctionKind::Method(_)
        | FunctionKind::AsyncMethod(_)
        | FunctionKind::MethodGetter(_)
        | FunctionKind::MethodSetter(_)
        | FunctionKind::Static(_)
        | FunctionKind::AsyncStatic(_)
        | FunctionKind::StaticGetter(_)
        | FunctionKind::StaticSetter(_)
        | FunctionKind::Constructor(_) => {}
    }
}

#[allow(clippy::too_many_lines)]
fn check_def(
    contract: &Contract,
    id: TypeId,
    at: &str,
    reason: Option<&str>,
    span: Span,
    sink: &mut Sink<'_>,
) {
    let def = &contract.resolve.types[id];
    let def_span = pick_span(def.span, span);
    match &def.kind {
        TypeDefKind::Record(r) => {
            // `wit-parser` already rejects empty records; kept in case a bump stops doing so.
            if r.fields.is_empty() {
                sink.refuse_with_reason(
                    codes::SUBSET_EMPTY_RECORD,
                    at,
                    "empty records are not supported; add a field or use a variant case without payload",
                    reason,
                    def_span,
                );
            }
            check_scope_case_collisions(
                at,
                "field",
                r.fields
                    .iter()
                    .map(|field| (field.name.as_str(), pick_span(field.span, def_span))),
                |name| format!("{at}.{name}"),
                sink,
            );
            for field in &r.fields {
                let field_span = pick_span(field.span, def_span);
                check_type(
                    contract,
                    &field.ty,
                    &format!("{at}.{}", field.name),
                    reason,
                    field_span,
                    sink,
                );
            }
        }
        TypeDefKind::Variant(v) => {
            check_scope_case_collisions(
                at,
                "case",
                v.cases
                    .iter()
                    .map(|case| (case.name.as_str(), pick_span(case.span, def_span))),
                |name| format!("{at}.{name}"),
                sink,
            );
            for case in &v.cases {
                if let Some(ty) = &case.ty {
                    let case_span = pick_span(case.span, def_span);
                    check_type(
                        contract,
                        ty,
                        &format!("{at}.{}", case.name),
                        reason,
                        case_span,
                        sink,
                    );
                }
            }
        }
        TypeDefKind::Enum(e) => {
            check_scope_case_collisions(
                at,
                "case",
                e.cases
                    .iter()
                    .map(|case| (case.name.as_str(), pick_span(case.span, def_span))),
                |name| format!("{at}.{name}"),
                sink,
            );
        }
        TypeDefKind::Flags(f) => {
            check_scope_case_collisions(
                at,
                "flag",
                f.flags
                    .iter()
                    .map(|flag| (flag.name.as_str(), pick_span(flag.span, def_span))),
                |name| format!("{at}.{name}"),
                sink,
            );
        }
        TypeDefKind::Option(ty) | TypeDefKind::List(ty) | TypeDefKind::Type(ty) => {
            check_type(contract, ty, at, reason, def_span, sink);
        }
        TypeDefKind::Result(r) => {
            for ty in [&r.ok, &r.err].into_iter().flatten() {
                check_type(contract, ty, at, reason, def_span, sink);
            }
        }
        TypeDefKind::Tuple(_) => {
            sink.refuse_with_reason(
                codes::SUBSET_TUPLE,
                at,
                "tuples are not supported; use a named record",
                reason,
                def_span,
            );
        }
        TypeDefKind::Resource | TypeDefKind::Handle(_) => sink.refuse_with_reason(
            codes::SUBSET_RESOURCE,
            at,
            "resources and handles are not supported; contract operations are pure functions over values",
            reason,
            def_span,
        ),
        TypeDefKind::Map(key, value) => {
            if !is_wit_map_key(key) {
                sink.refuse_with_reason(
                    codes::SUBSET_MAP,
                    at,
                    "`map` keys must be `bool`, an integer, `char`, or `string`",
                    reason,
                    def_span,
                );
            }
            check_type(contract, key, at, reason, def_span, sink);
            check_type(contract, value, at, reason, def_span, sink);
        }
        TypeDefKind::FixedLengthList(ty, len) => {
            if *len == 0 || *len > crate::model::MAX_FIXED_LIST_LEN {
                sink.refuse_with_reason(
                    codes::SUBSET_FIXED_LENGTH_LIST,
                    at,
                    format!(
                        "fixed-length list length `{len}` must be between 1 and {}; use `list` for unbounded or empty lists",
                        crate::model::MAX_FIXED_LIST_LEN
                    ),
                    reason,
                    def_span,
                );
            }
            check_type(contract, ty, at, reason, def_span, sink);
        }
        TypeDefKind::Future(_) | TypeDefKind::Stream(_) => {
            sink.refuse_with_reason(
                codes::SUBSET_FUTURE_OR_STREAM,
                at,
                "`future` and `stream` are not supported; the contract is synchronous",
                reason,
                def_span,
            );
        }
        TypeDefKind::Unknown => sink.refuse_with_reason(
            codes::SUBSET_UNSUPPORTED,
            at,
            "unresolved type",
            reason,
            def_span,
        ),
    }
}

fn check_type(
    contract: &Contract,
    ty: &Type,
    at: &str,
    reason: Option<&str>,
    span: Span,
    sink: &mut Sink<'_>,
) {
    match ty {
        // Supported primitives. Listed explicitly so a new `wit-parser` type variant is a
        // compile error rather than silently passing the subset.
        Type::Bool
        | Type::U8
        | Type::U16
        | Type::U32
        | Type::U64
        | Type::S8
        | Type::S16
        | Type::S32
        | Type::S64
        | Type::F32
        | Type::F64
        | Type::Char
        | Type::String => {}
        Type::ErrorContext => {
            sink.refuse_with_reason(
                codes::SUBSET_ERROR_CONTEXT,
                at,
                "`error-context` is not supported",
                reason,
                span,
            );
        }
        Type::Id(id) => {
            let def = &contract.resolve.types[*id];
            // Anonymous types (`option<T>`, `list<T>`, `result<..>`, tuples) are checked
            // where they are used. A named dependency type is resolved by wit-parser through
            // the standard deps layout; its own package is responsible for its subset and the
            // model records the qualified package identity.
            if def.name.is_none() {
                check_def(contract, *id, at, reason, span, sink);
            }
        }
    }
}

/// Detect pairs of distinct WIT names in one package that map to one generated Rust, protobuf, or
/// `prost` identifier, as well as reserved generated names (`functions`, `witforge_value`,
/// `UNSPECIFIED`, `Empty`, and interface-trait collisions).
#[allow(clippy::too_many_lines)]
fn check_package_generated_collisions(
    resolve: &wit_parser::Resolve,
    pkg: &str,
    package: &wit_parser::Package,
    sink: &mut Sink<'_>,
) {
    let mut rust_mods: BTreeMap<String, String> = BTreeMap::new();
    let mut rust_traits: BTreeMap<String, String> = BTreeMap::new();
    let mut proto_types: BTreeMap<String, String> = BTreeMap::new();
    let mut prost_types: BTreeMap<String, String> = BTreeMap::new();
    let mut go_proto_types: BTreeMap<String, String> = BTreeMap::new();
    let mut prost_oneof_mods: BTreeMap<String, String> = BTreeMap::new();
    let mut proto_enum_vals: BTreeMap<String, String> = BTreeMap::new();

    proto_types.insert("Empty".to_string(), "shared Empty message".to_string());
    prost_types.insert(
        ident::prost_type_ident("Empty"),
        "shared Empty message".to_string(),
    );
    go_proto_types.insert(
        crate::emit::go_glue::protoc_go_camel_case("Empty"),
        "shared Empty message".to_string(),
    );

    let mut seen_ifaces_ci: HashSet<String> = HashSet::new();
    let mut iface_ci_dups: HashSet<String> = HashSet::new();
    for (iface_name, &iface_id) in &package.interfaces {
        let iface = &resolve.interfaces[iface_id];
        if !seen_ifaces_ci.insert(iface_name.to_ascii_lowercase()) {
            iface_ci_dups.insert(iface_name.clone());
        }
        if let Ok(module) = ident::plain_ident(&ident::plain_snake(iface_name), "interface module")
            && let Some(prev) = rust_mods.insert(module.clone(), iface_name.clone())
            && !prev.eq_ignore_ascii_case(iface_name)
        {
            sink.refuse_collision(
                format!("{pkg}/{iface_name}"),
                format!(
                    "interfaces `{prev}` and `{iface_name}` both map to Rust module `{module}`"
                ),
                iface.span,
            );
        }
        if !iface.functions.is_empty()
            && let Ok(trait_name) =
                ident::plain_ident(&ident::plain_pascal(iface_name), "interface trait")
            && let Some(prev) = rust_traits.insert(trait_name.clone(), iface_name.clone())
            && !prev.eq_ignore_ascii_case(iface_name)
        {
            sink.refuse_collision(
                format!("{pkg}/{iface_name}"),
                format!(
                    "interfaces `{prev}` and `{iface_name}` both map to Rust trait `{trait_name}`"
                ),
                iface.span,
            );
        }
    }

    // Pass 1: Named types across all interfaces (matching proto.rs declaration registration order).
    for (iface_name, &iface_id) in &package.interfaces {
        let iface = &resolve.interfaces[iface_id];
        let iface_trait_name = if iface.functions.is_empty() {
            None
        } else {
            ident::plain_ident(&ident::plain_pascal(iface_name), "interface trait").ok()
        };
        let mut iface_rust_types: BTreeMap<String, String> = BTreeMap::new();
        let mut iface_glue_mods: BTreeMap<String, String> = BTreeMap::new();
        for (type_name, &type_id) in &iface.types {
            let def = &resolve.types[type_id];
            let def_span = pick_span(def.span, iface.span);
            let location = format!("{pkg}/{iface_name}/{type_name}");
            if let Ok(rust_ty) = ident::plain_ident(&ident::plain_pascal(type_name), "type") {
                if let Some(prev) = iface_rust_types.insert(rust_ty.clone(), type_name.clone())
                    && !prev.eq_ignore_ascii_case(type_name)
                {
                    sink.refuse_collision(
                        &location,
                        format!(
                            "types `{prev}` and `{type_name}` in `{iface_name}` both map to Rust type `{rust_ty}`"
                        ),
                        def_span,
                    );
                }
                if iface_trait_name.as_deref() == Some(rust_ty.as_str()) {
                    sink.refuse_collision(
                        &location,
                        format!(
                            "interface `{iface_name}`: Rust name `{rust_ty}` is produced by both \
                             `trait {iface_name}` and `type {type_name}`"
                        ),
                        def_span,
                    );
                }
            }
            if let Ok(glue_mod) = ident::plain_ident(&ident::plain_snake(type_name), "type module")
            {
                if glue_mod == "functions" {
                    sink.refuse_collision(
                        &location,
                        format!(
                            "type `{iface_name}/{type_name}` maps to reserved glue module `functions`"
                        ),
                        def_span,
                    );
                }
                if let Some(prev) = iface_glue_mods.insert(glue_mod.clone(), type_name.clone())
                    && !prev.eq_ignore_ascii_case(type_name)
                {
                    sink.refuse_collision(
                        &location,
                        format!(
                            "types `{prev}` and `{type_name}` in `{iface_name}` both map to glue module `{glue_mod}`"
                        ),
                        def_span,
                    );
                }
            }

            if def.owner != TypeOwner::Interface(iface_id) {
                continue;
            }
            let named_opt_wrapper_kind = match &def.kind {
                TypeDefKind::Option(payload) => match resolve_wit_alias(resolve, payload) {
                    Type::Id(inner_id) => match &resolve.types[*inner_id].kind {
                        TypeDefKind::List(elem) | TypeDefKind::FixedLengthList(elem, _)
                            if !is_wit_u8(resolve, elem) =>
                        {
                            Some(false)
                        }
                        TypeDefKind::Map(..) => Some(false),
                        TypeDefKind::Option(_) => Some(true),
                        _ => None,
                    },
                    _ => None,
                },
                _ => None,
            };
            let produces_proto_type = matches!(
                def.kind,
                TypeDefKind::Record(_)
                    | TypeDefKind::Variant(_)
                    | TypeDefKind::Enum(_)
                    | TypeDefKind::Flags(_)
                    | TypeDefKind::Result(_)
            ) || named_opt_wrapper_kind.is_some();
            if !produces_proto_type {
                continue;
            }
            let proto_name = ident::proto_type_name(iface_name, type_name);
            let prost_name = ident::prost_type_ident(&proto_name);
            let go_proto_name = crate::emit::go_glue::protoc_go_camel_case(&proto_name);
            let source = format!("type {iface_name}/{type_name}");
            if let Some(prev) = proto_types.insert(proto_name.clone(), source.clone()) {
                sink.refuse_collision(
                    &location,
                    format!("proto name `{proto_name}` is produced by both {prev} and {source}"),
                    def_span,
                );
            } else if let Some(prev) = prost_types.insert(prost_name.clone(), source.clone()) {
                sink.refuse_collision(
                    &location,
                    format!(
                        "prost type name `{prost_name}` is produced by both {prev} and {source}"
                    ),
                    def_span,
                );
            } else if let Some(prev) = go_proto_types.insert(go_proto_name.clone(), source.clone())
            {
                sink.refuse_collision(
                    &location,
                    format!(
                        "protoc-gen-go type name `{go_proto_name}` is produced by both {prev} and {source}"
                    ),
                    def_span,
                );
            }
            if matches!(def.kind, TypeDefKind::Variant(_) | TypeDefKind::Result(_))
                || named_opt_wrapper_kind == Some(true)
            {
                let oneof_mod = ident::prost_oneof_module(&proto_name);
                if let Some(prev) = prost_oneof_mods.insert(oneof_mod.clone(), source.clone()) {
                    sink.refuse_collision(
                        &location,
                        format!(
                            "prost oneof module `{oneof_mod}` is produced by both {prev} and {source}"
                        ),
                        def_span,
                    );
                }
            }
            if let TypeDefKind::Variant(variant) = &def.kind {
                let mut rust_cases: BTreeMap<String, String> = BTreeMap::new();
                let mut proto_fields: BTreeMap<String, String> = BTreeMap::new();
                let mut prost_cases: BTreeMap<String, String> = BTreeMap::new();
                for case in &variant.cases {
                    let case_span = pick_span(case.span, def_span);
                    let proto_case = ident::proto_field_name(&case.name);
                    if proto_case == "witforge_value" {
                        sink.refuse_collision(
                            format!("{location}.{}", case.name),
                            format!(
                                "variant `{iface_name}/{type_name}`: case `{proto_case}` collides with oneof name"
                            ),
                            case_span,
                        );
                    }
                    if let Some(prev) = proto_fields.insert(proto_case.clone(), case.name.clone())
                        && !prev.eq_ignore_ascii_case(&case.name)
                    {
                        sink.refuse_collision(
                            format!("{location}.{}", case.name),
                            format!(
                                "variant `{iface_name}/{type_name}`: cases `{prev}` and `{}` both map to proto field `{proto_case}`",
                                case.name
                            ),
                            case_span,
                        );
                    }
                    if let Ok(rust_case) =
                        ident::plain_ident(&ident::plain_pascal(&case.name), "variant case")
                        && let Some(prev) = rust_cases.insert(rust_case.clone(), case.name.clone())
                        && !prev.eq_ignore_ascii_case(&case.name)
                    {
                        sink.refuse_collision(
                            format!("{location}.{}", case.name),
                            format!(
                                "variant `{iface_name}/{type_name}`: cases `{prev}` and `{}` both map to Rust variant `{rust_case}`",
                                case.name
                            ),
                            case_span,
                        );
                    }
                    let prost_var = ident::prost_oneof_variant(&proto_case);
                    if let Some(prev) = prost_cases.insert(prost_var.clone(), case.name.clone())
                        && !prev.eq_ignore_ascii_case(&case.name)
                    {
                        sink.refuse_collision(
                            format!("{location}.{}", case.name),
                            format!(
                                "variant `{iface_name}/{type_name}`: cases `{prev}` and `{}` both map to prost variant `{prost_var}`",
                                case.name
                            ),
                            case_span,
                        );
                    }
                }
            }
            if let TypeDefKind::Record(record) = &def.kind {
                check_member_name_collisions(
                    &format!("record `{iface_name}/{type_name}`"),
                    "field",
                    record
                        .fields
                        .iter()
                        .map(|field| (field.name.as_str(), pick_span(field.span, def_span))),
                    |name| format!("{location}.{name}"),
                    sink,
                );
                check_prost_field_accessor_collisions(
                    resolve,
                    &location,
                    &format!("record `{iface_name}/{type_name}`"),
                    "field",
                    record.fields.iter().map(|field| {
                        (
                            field.name.as_str(),
                            &field.ty,
                            pick_span(field.span, def_span),
                        )
                    }),
                    |name| format!("{location}.{name}"),
                    sink,
                );
            }
            if let TypeDefKind::Flags(flags) = &def.kind {
                check_member_name_collisions(
                    &format!("flags `{iface_name}/{type_name}`"),
                    "flag",
                    flags
                        .flags
                        .iter()
                        .map(|flag| (flag.name.as_str(), pick_span(flag.span, def_span))),
                    |name| format!("{location}.{name}"),
                    sink,
                );
            }
            if let TypeDefKind::Enum(enum_def) = &def.kind {
                let zero = ident::proto_enum_zero_value(&proto_name);
                let mut rust_cases: BTreeMap<String, String> = BTreeMap::new();
                let mut enum_prost_variants: BTreeMap<String, String> = BTreeMap::new();
                enum_prost_variants
                    .insert(ident::prost_enum_variant(&proto_name, &zero), zero.clone());
                if let Some(prev) = proto_enum_vals.insert(zero.clone(), source.clone()) {
                    sink.refuse_collision(
                        &location,
                        format!(
                            "proto enum value `{zero}` is produced by both {prev} and {source}"
                        ),
                        def_span,
                    );
                }
                for case in &enum_def.cases {
                    let case_span = pick_span(case.span, def_span);
                    if ident::proto_upper_snake(&case.name) == "UNSPECIFIED" {
                        sink.refuse_collision(
                            format!("{location}.{}", case.name),
                            format!("enum `{iface_name}/{type_name}`: case maps to UNSPECIFIED"),
                            case_span,
                        );
                    }
                    if let Ok(rust_case) =
                        ident::plain_ident(&ident::plain_pascal(&case.name), "enum case")
                        && let Some(prev) = rust_cases.insert(rust_case.clone(), case.name.clone())
                        && !prev.eq_ignore_ascii_case(&case.name)
                    {
                        sink.refuse_collision(
                            format!("{location}.{}", case.name),
                            format!(
                                "enum `{iface_name}/{type_name}`: cases `{prev}` and `{}` both map to Rust variant `{rust_case}`",
                                case.name
                            ),
                            case_span,
                        );
                    }
                    let val = ident::proto_enum_value_name(&proto_name, &case.name);
                    let case_source = format!("enum {iface_name}/{type_name}.{}", case.name);
                    if let Some(prev) = proto_enum_vals.insert(val.clone(), case_source.clone()) {
                        sink.refuse_collision(
                            format!("{location}.{}", case.name),
                            format!(
                                "proto enum value `{val}` is produced by both {prev} and {case_source}"
                            ),
                            case_span,
                        );
                    }
                    let prost_var = ident::prost_enum_variant(&proto_name, &val);
                    if let Some(prev) = enum_prost_variants.insert(prost_var.clone(), val.clone()) {
                        sink.refuse_collision(
                            format!("{location}.{}", case.name),
                            format!(
                                "enum `{iface_name}/{type_name}`: proto values `{prev}` and `{val}` both map to prost variant `{prost_var}`"
                            ),
                            case_span,
                        );
                    }
                }
            }
        }
    }

    // Pass 2: Function request/response messages and method enums across all interfaces.
    for (iface_name, &iface_id) in &package.interfaces {
        let iface = &resolve.interfaces[iface_id];
        let iface_ci_dup = iface_ci_dups.contains(iface_name);
        let method_enum = ident::proto_method_enum_name(iface_name);
        let method_prost = ident::prost_type_ident(&method_enum);
        let method_source = format!("method enum {iface_name}");
        let mut rust_funcs: BTreeMap<String, String> = BTreeMap::new();
        let method_zero = ident::proto_method_enum_zero_value(iface_name);
        let mut method_prost_variants: BTreeMap<String, String> = BTreeMap::new();
        method_prost_variants.insert(
            ident::prost_enum_variant(&method_enum, &method_zero),
            method_zero.clone(),
        );
        for (func_name, func) in &iface.functions {
            if !matches!(func.kind, FunctionKind::Freestanding) {
                continue;
            }
            let func_span = pick_span(func.span, iface.span);
            let fn_location = format!("{pkg}/{iface_name}#{func_name}");
            for suffix in ["Request", "Response"] {
                let msg_name = ident::proto_function_message_name(iface_name, func_name, suffix);
                let msg_prost = ident::prost_type_ident(&msg_name);
                let msg_go = crate::emit::go_glue::protoc_go_camel_case(&msg_name);
                let msg_source = format!("function {iface_name}/{func_name} {suffix}");
                if let Some(prev) = proto_types.insert(msg_name.clone(), msg_source.clone()) {
                    sink.refuse_collision(
                        &fn_location,
                        format!(
                            "proto name `{msg_name}` is produced by both {prev} and {msg_source}"
                        ),
                        func_span,
                    );
                } else if let Some(prev) = prost_types.insert(msg_prost.clone(), msg_source.clone())
                {
                    sink.refuse_collision(
                        &fn_location,
                        format!(
                            "prost type name `{msg_prost}` is produced by both {prev} and {msg_source}"
                        ),
                        func_span,
                    );
                } else if let Some(prev) = go_proto_types.insert(msg_go.clone(), msg_source.clone())
                {
                    sink.refuse_collision(
                        &fn_location,
                        format!(
                            "protoc-gen-go type name `{msg_go}` is produced by both {prev} and {msg_source}"
                        ),
                        func_span,
                    );
                }
            }
            if ident::proto_upper_snake(func_name) == "UNSPECIFIED" {
                sink.refuse_collision(
                    &fn_location,
                    format!(
                        "function `{iface_name}.{func_name}` maps to a reserved or UNSPECIFIED method value"
                    ),
                    func_span,
                );
            }
            if let Ok(rust_fn) = ident::plain_ident(&ident::plain_snake(func_name), "function")
                && let Some(prev) = rust_funcs.insert(rust_fn.clone(), func_name.clone())
                && !prev.eq_ignore_ascii_case(func_name)
            {
                sink.refuse_collision(
                    &fn_location,
                    format!(
                        "functions `{prev}` and `{func_name}` in `{iface_name}` both map to Rust function `{rust_fn}`"
                    ),
                    func_span,
                );
            }
            let val = ident::proto_method_enum_value(iface_name, func_name);
            let source = format!("function {iface_name}#{func_name}");
            if let Some(prev) = proto_enum_vals.insert(val.clone(), source.clone()) {
                sink.refuse_collision(
                    &fn_location,
                    format!("proto enum value `{val}` is produced by both {prev} and {source}"),
                    func_span,
                );
            }
            let prost_var = ident::prost_enum_variant(&method_enum, &val);
            if let Some(prev) = method_prost_variants.insert(prost_var.clone(), val.clone()) {
                sink.refuse_collision(
                    &fn_location,
                    format!(
                        "method values `{prev}` and `{val}` in `{iface_name}` both map to prost variant `{prost_var}`"
                    ),
                    func_span,
                );
            }
            check_member_name_collisions(
                &format!("function `{iface_name}#{func_name}`"),
                "parameter",
                func.params
                    .iter()
                    .map(|param| (param.name.as_str(), pick_span(param.span, func_span))),
                |name| format!("{fn_location}({name})"),
                sink,
            );
            check_prost_field_accessor_collisions(
                resolve,
                &fn_location,
                &format!("function `{iface_name}#{func_name}`"),
                "parameter",
                func.params.iter().map(|param| {
                    (
                        param.name.as_str(),
                        &param.ty,
                        pick_span(param.span, func_span),
                    )
                }),
                |name| format!("{fn_location}({name})"),
                sink,
            );
        }
        if !iface_ci_dup {
            let method_go = crate::emit::go_glue::protoc_go_camel_case(&method_enum);
            if let Some(prev) = proto_types.insert(method_enum.clone(), method_source.clone()) {
                sink.refuse_collision(
                    format!("{pkg}/{iface_name}"),
                    format!(
                        "proto name `{method_enum}` is produced by both {prev} and {method_source}"
                    ),
                    iface.span,
                );
            } else if let Some(prev) =
                prost_types.insert(method_prost.clone(), method_source.clone())
            {
                sink.refuse_collision(
                    format!("{pkg}/{iface_name}"),
                    format!(
                        "prost type name `{method_prost}` is produced by both {prev} and {method_source}"
                    ),
                    iface.span,
                );
            } else if let Some(prev) =
                go_proto_types.insert(method_go.clone(), method_source.clone())
            {
                sink.refuse_collision(
                    format!("{pkg}/{iface_name}"),
                    format!(
                        "protoc-gen-go type name `{method_go}` is produced by both {prev} and {method_source}"
                    ),
                    iface.span,
                );
            }
            if let Some(prev) =
                proto_enum_vals.insert(method_zero.clone(), format!("method enum {iface_name}"))
            {
                sink.refuse_collision(
                    format!("{pkg}/{iface_name}"),
                    format!(
                        "proto enum value `{method_zero}` is produced by both {prev} and method enum {iface_name}"
                    ),
                    iface.span,
                );
            }
        }
    }
}

fn check_member_name_collisions<'a>(
    parent_label: &str,
    member_kind: &str,
    names: impl IntoIterator<Item = (&'a str, Span)>,
    location_for: impl Fn(&str) -> String,
    sink: &mut Sink<'_>,
) {
    let mut rust_names: BTreeMap<String, String> = BTreeMap::new();
    let mut proto_names: BTreeMap<String, String> = BTreeMap::new();
    let mut prost_names: BTreeMap<String, String> = BTreeMap::new();
    for (name, span) in names {
        if let Ok(rust_name) = ident::plain_ident(&ident::plain_snake(name), member_kind)
            && let Some(prev) = rust_names.insert(rust_name.clone(), name.to_string())
            && !prev.eq_ignore_ascii_case(name)
        {
            sink.refuse_collision(
                location_for(name),
                format!(
                    "{parent_label}: {member_kind}s `{prev}` and `{name}` both map to Rust name `{rust_name}`"
                ),
                span,
            );
        }
        let proto_name = ident::proto_field_name(name);
        if let Some(prev) = proto_names.insert(proto_name.clone(), name.to_string())
            && !prev.eq_ignore_ascii_case(name)
        {
            sink.refuse_collision(
                location_for(name),
                format!(
                    "{parent_label}: {member_kind}s `{prev}` and `{name}` both map to proto field `{proto_name}`"
                ),
                span,
            );
        }
        let prost_name = ident::prost_field_ident(&proto_name);
        if let Some(prev) = prost_names.insert(prost_name.clone(), name.to_string())
            && !prev.eq_ignore_ascii_case(name)
        {
            sink.refuse_collision(
                location_for(name),
                format!(
                    "{parent_label}: {member_kind}s `{prev}` and `{name}` both map to prost field `{prost_name}`"
                ),
                span,
            );
        }
    }
}

/// Detect collisions between inherent accessor methods generated by `prost-derive` 0.14 on a
/// single protobuf message (record or function request).
fn check_prost_field_accessor_collisions<'a>(
    resolve: &wit_parser::Resolve,
    _parent_location: &str,
    parent_label: &str,
    member_kind: &str,
    fields: impl IntoIterator<Item = (&'a str, &'a Type, Span)>,
    location_for: impl Fn(&str) -> String,
    sink: &mut Sink<'_>,
) {
    let mut accessors: BTreeMap<String, String> = BTreeMap::new();
    for (name, ty, span) in fields {
        let proto_field = ident::proto_field_name(name);
        let kind = resolve_prost_field_accessor_kind(resolve, ty);
        if kind == ident::ProstFieldAccessorKind::MapEnum
            && ident::prost_field_ident(&proto_field).starts_with("r#")
        {
            sink.refuse_collision(
                location_for(name),
                format!(
                    "{parent_label}: {member_kind} `{name}` has type `map<_, enum>` with Rust keyword name `{proto_field}`, which fails `prost-derive` accessor generation (`get_r#{proto_field}`); rename the {member_kind} or wrap the enum in a record"
                ),
                span,
            );
        }
        for accessor in ident::prost_field_accessors(&proto_field, kind) {
            if let Some(prev) = accessors.get(&accessor) {
                if !prev.eq_ignore_ascii_case(name) {
                    sink.refuse_collision(
                        location_for(name),
                        format!(
                            "{parent_label}: {member_kind}s `{prev}` and `{name}` both generate prost accessor `{accessor}`"
                        ),
                        span,
                    );
                }
            } else {
                accessors.insert(accessor, name.to_string());
            }
        }
    }
}

fn resolve_wit_alias<'a>(resolve: &'a wit_parser::Resolve, mut ty: &'a Type) -> &'a Type {
    while let Type::Id(id) = ty {
        match &resolve.types[*id].kind {
            TypeDefKind::Type(inner) => ty = inner,
            _ => break,
        }
    }
    ty
}

fn is_wit_u8(resolve: &wit_parser::Resolve, ty: &Type) -> bool {
    matches!(resolve_wit_alias(resolve, ty), Type::U8)
}

fn is_wit_enum(resolve: &wit_parser::Resolve, ty: &Type) -> bool {
    matches!(
        resolve_wit_alias(resolve, ty),
        Type::Id(id) if matches!(resolve.types[*id].kind, TypeDefKind::Enum(_))
    )
}

fn is_wit_map_key(ty: &Type) -> bool {
    matches!(
        ty,
        Type::Bool
            | Type::U8
            | Type::U16
            | Type::U32
            | Type::U64
            | Type::S8
            | Type::S16
            | Type::S32
            | Type::S64
            | Type::Char
            | Type::String
    )
}

fn is_wit_scalar_or_bytes(resolve: &wit_parser::Resolve, ty: &Type) -> bool {
    match resolve_wit_alias(resolve, ty) {
        Type::Bool
        | Type::U8
        | Type::U16
        | Type::U32
        | Type::U64
        | Type::S8
        | Type::S16
        | Type::S32
        | Type::S64
        | Type::F32
        | Type::F64
        | Type::Char
        | Type::String => true,
        Type::Id(id) => match &resolve.types[*id].kind {
            TypeDefKind::List(element) | TypeDefKind::FixedLengthList(element, _) => {
                is_wit_u8(resolve, element)
            }
            _ => false,
        },
        Type::ErrorContext => false,
    }
}

fn resolve_prost_field_accessor_kind(
    resolve: &wit_parser::Resolve,
    ty: &Type,
) -> ident::ProstFieldAccessorKind {
    match resolve_wit_alias(resolve, ty) {
        Type::Bool
        | Type::U8
        | Type::U16
        | Type::U32
        | Type::U64
        | Type::S8
        | Type::S16
        | Type::S32
        | Type::S64
        | Type::F32
        | Type::F64
        | Type::Char
        | Type::String => ident::ProstFieldAccessorKind::OptionalScalar,
        Type::Id(id) => match &resolve.types[*id].kind {
            TypeDefKind::Enum(_) => ident::ProstFieldAccessorKind::OptionalEnum,
            TypeDefKind::List(element) | TypeDefKind::FixedLengthList(element, _) => {
                if is_wit_u8(resolve, element) {
                    ident::ProstFieldAccessorKind::OptionalScalar
                } else if is_wit_enum(resolve, element) {
                    ident::ProstFieldAccessorKind::RepeatedEnum
                } else {
                    ident::ProstFieldAccessorKind::None
                }
            }
            TypeDefKind::Map(_, value) => {
                if is_wit_enum(resolve, value) {
                    ident::ProstFieldAccessorKind::MapEnum
                } else {
                    ident::ProstFieldAccessorKind::None
                }
            }
            TypeDefKind::Option(payload) => {
                if is_wit_enum(resolve, payload) {
                    ident::ProstFieldAccessorKind::OptionalEnum
                } else if is_wit_scalar_or_bytes(resolve, payload) {
                    ident::ProstFieldAccessorKind::OptionalScalar
                } else {
                    ident::ProstFieldAccessorKind::None
                }
            }
            _ => ident::ProstFieldAccessorKind::None,
        },
        Type::ErrorContext => ident::ProstFieldAccessorKind::None,
    }
}

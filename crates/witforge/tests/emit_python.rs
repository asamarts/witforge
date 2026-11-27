//! Tier 1 offline + live `python3` runtime and typing verification tests for
//! `witforge::emit::python` (`W13-004`).

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use witforge::emit::output::{GeneratedFileStatus, HeaderStyle};
use witforge::emit::python::{self, PythonOptions};
use witforge::{conformance, is_contract_error, load, model};

fn fixture(rel: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(rel)
}

fn scratch(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("witforge-py-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn lock_version_dir(dir: &Path, against: Option<&Path>) {
    let baseline = match against {
        Some(prev) => witforge::workflow::Baseline::Against(prev.to_path_buf()),
        None => witforge::workflow::Baseline::First,
    };
    let report = witforge::workflow::lock(
        &witforge::workflow::VersionOptions::new(dir.to_path_buf()).with_baseline(baseline),
    )
    .unwrap();
    assert!(
        report.is_ok(),
        "lock failed for {}: {report:?}",
        dir.display()
    );
    report.write().unwrap();
}

/// Find a `python3` binary with version >= 3.11 (for `enum.StrEnum`, `typing.Never`,
/// `@dataclass(slots=True)`).
fn find_python311() -> Option<PathBuf> {
    let py = PathBuf::from("python3");
    let out = Command::new(&py)
        .args([
            "-c",
            "import sys; sys.exit(0 if sys.version_info >= (3, 11) else 1)",
        ])
        .output()
        .ok()?;
    out.status.success().then_some(py)
}

#[allow(clippy::too_many_lines)]
fn write_all_constructs_contract(wit_root: &Path) {
    let v01 = wit_root.join("v0.1");
    let dep = v01.join("deps/dep-shared");
    fs::create_dir_all(&dep).unwrap();
    fs::write(
        dep.join("shared.wit"),
        r"package acme:shared@0.1.0;

/// Shared dependency types.
interface common {
  type dep-byte = u8;
  type dep-maybe = option<u32>;

  record tag {
    key: string,
    code: u16,
  }

  enum level {
    low,
    high,
  }
}

/// Dependency interface whose name is a Python keyword.
interface %class {
  record dep-entry {
    name: string,
  }
}

/// Dependency interface whose name matches Windows `CON`.
interface con {
  use common.{tag};

  record dep-conn {
    meta: tag,
  }
}
",
    )
    .unwrap();

    fs::write(
        v01.join("contract.wit"),
        r#"package acme:showcase@0.1.0;

/// Core types exercising every supported WIT construct and Python 3.11+ edge case.
@since(version = 0.1.0)
interface types {
  use acme:shared/common@0.1.0.{dep-byte, dep-maybe, tag, level};
  use acme:shared/%class@0.1.0.{dep-entry};
  use acme:shared/con@0.1.0.{dep-conn};

  // Forward-referenced aliases declared BEFORE `color` to prove topological alias ordering.
  type alias-chain-top = alias-chain-mid;
  type alias-chain-mid = list<color>;
  type err-only = result<_, color>;
  type byte = u8;
  type id-list = list<u64>;
  /**
   * Fixed 4-byte token header.
   */
  type fixed-bytes = list<byte, 4>;
  type fixed-nums = list<u16, 3>;
  type maybe-u16 = option<u16>;
  type alias-maybe = maybe-u16;
  type nested-maybe = option<option<u16>>;
  type triple-maybe = option<option<option<u16>>>;
  type aliased-nested-maybe = option<maybe-u16>;
  type deep-aliased-maybe = option<alias-maybe>;
  type dep-nested-maybe = option<dep-maybe>;
  type lookup = map<string, u32>;
  type outcome = result<u16, string>;
  type ok-only = result<u8>;
  type bare-res = result;

  /// Color enum with docs containing """triple quotes""", "quotes", and trailing \ backslash.
  @since(version = 0.1.0)
  @deprecated(version = 0.1.0)
  enum color {
    /// "Warm" red ("""quoted""" and C:\red\).
    red,
    /// """
    in-progress,
    /// "
    blue,
  }

  enum keyword-cases {
    %type,
    %class,
    %from,
    %none,
    %true,
    %false,
    name,
    value,
  }

  flags perms {
    read,
    write,
    %class,
    %from,
    self,
    mro,
  }

  /// Record with `mro` first (must escape to `mro_` so `@dataclass` never sees `type.mro`).
  record mro-first {
    mro: u32,
    name: string,
  }

  /// Record with `mro` as the sole field.
  record mro-only {
    mro: u32,
  }

  /// Record exercising builtin-named fields (`bool`, `int`, `float`, `str`, `bytes`, `tuple`),
  /// `self`, `mro`, and Python keywords.
  record builtin-fields {
    %bool: bool,
    int: u32,
    float: f64,
    str: string,
    bytes: list<u8>,
    %tuple: list<u16>,
    self: u8,
    mro: u8,
    %class: string,
    %from: bool,
    %match: u16,
    %type: u32,
  }

  /// Record with every primitive, container, keyword field, acronyms/digits, and foreign types.
  record kitchen-sink {
    a-bool: bool,
    a-u8: u8,
    a-u16: u16,
    a-u32: u32,
    a-u64: u64,
    a-s8: s8,
    a-s16: s16,
    a-s32: s32,
    a-s64: s64,
    a-f32: f32,
    a-f64: f64,
    a-char: char,
    a-string: string,
    raw-bytes: list<u8>,
    alias-bytes: list<byte>,
    dep-alias-bytes: list<dep-byte>,
    fixed-raw: list<u8, 4>,
    fixed-nums: list<u16, 3>,
    nested-bytes: list<list<u8>>,
    nested-alias-bytes: list<list<byte>>,
    nested-fixed-bytes: list<list<u8, 4>>,
    nested-nums: list<list<u16>>,
    nested-fixed-nums: list<list<u16, 2>>,
    nested-options: list<option<u8>>,
    nested-results: list<result<u8, color>>,
    nested-named-results: list<outcome>,
    opt-u8: option<u8>,
    nested-opt: option<option<u16>>,
    triple-opt: option<option<option<u16>>>,
    aliased-opt: option<maybe-u16>,
    dep-opt: option<dep-maybe>,
    str-map: map<string, u8>,
    u64-map: map<u64, s64>,
    char-map: map<char, color>,
    nested-maps: list<map<u8, u16>>,
    both-res: result<u16, string>,
    only-ok: result<u8>,
    only-err: result<_, color>,
    bare: result,
    foreign-tag: tag,
    foreign-level: level,
    foreign-class: dep-entry,
    foreign-con: dep-conn,
    http-api: string,
    url: string,
    x-kind: u8,
    foo2-bar: u16,
    kw-case: keyword-cases,
    chain: alias-chain-top,
  }

  variant choice {
    /// Unit variant case.
    unit-case,
    small(u8),
    nested(kitchen-sink),
    fixed(list<u16, 2>),
  }

  // Variant case name `<Variant><Case>` colliding with top-level record `foo-bar-baz`!
  record foo-bar-baz {
    id: u32,
  }

  variant foo {
    bar-baz(u8),
    other,
  }
}

/// Service interface with functions, keyword/builtin/self parameters & methods, and cross-interface types.
interface api {
  use types.{kitchen-sink, choice, color};
  use %class.{entry};
  use con.{conn};
  use %from.{origin};
  use %type.{kind-item};
  use acme:shared/common@0.1.0.{tag};

  /// Transform input and return a result.
  @unstable(feature = preview-api)
  process: func(input: kitchen-sink, self: choice, %class: tag) -> result<color, string>;

  /// Void-returning function.
  notify: func(label: string, %return: u8);

  /// Keyword-escaped and builtin-named methods referencing escaped sibling interfaces.
  %from: func(a: entry, b: conn) -> origin;
  %type: func(k: kind-item) -> bool;
  mro: func() -> u32;
  str: func(self: string, other: string) -> string;
}

/// Empty interface emits `__all__ = ()`.
interface empty {}

/// Interfaces whose names are Python keywords or Windows reserved stems.
interface %class {
  record entry {
    name: string,
  }
}

interface %from {
  record origin {
    ok: bool,
  }
}

interface %type {
  record kind-item {
    code: u16,
  }
}

interface con {
  record conn {
    id: u32,
  }
}

interface nul {
  record nothing {
    flag: bool,
  }
}

/// Interface declaring types that shadow `Option`, `Ok`, `Err`, `Mapping`, `StrEnum`,
/// `Protocol`, `TypeAlias`, `None`, `True`, and `False`.
interface shadow {
  record %option {
    chosen: bool,
  }

  record ok {
    yes: bool,
  }

  record err {
    code: u32,
  }

  record mapping {
    size: u32,
  }

  record str-enum {
    label: string,
  }

  record protocol {
    ver: u8,
  }

  record type-alias {
    target: string,
  }

  record %none {
    empty: bool,
  }

  record %true {
    yes: bool,
  }

  record %false {
    no: bool,
  }

  enum tone {
    loud,
    soft,
  }

  type my-opt = option<option<u32>>;
  type my-res = result<u32, string>;
  type my-map = map<string, u32>;

  record carrier {
    real-opt: option<option<u32>>,
    real-res: result<u32, string>,
    real-map: map<string, u32>,
    real-enum: tone,
    local-opt: %option,
    local-ok: ok,
    local-err: err,
    local-map: mapping,
    local-enum: str-enum,
    local-proto: protocol,
    local-alias: type-alias,
    local-none: %none,
    local-true: %true,
    local-false: %false,
  }

  check: func(c: carrier) -> result<ok, err>;
}

/// Plugin world exercising docs, deprecation, and import/export stability.
@since(version = 0.1.0)
@deprecated(version = 0.1.0)
world plugin {
  /// Shared import with stability metadata.
  @since(version = 0.1.0)
  import acme:shared/common@0.1.0;
  import types;
  @unstable(feature = beta-export)
  export api;
}
"#,
    )
    .unwrap();
    lock_version_dir(&v01, None);
}

#[allow(clippy::too_many_lines)]
fn write_three_major_dep_layout_contract(wit_root: &Path) {
    let base_wit = r"package acme:base@0.1.0;
interface core {
  record stamp {
    epoch: u64,
  }
}
";
    let const_wit = r"package acme:const@0.1.0;
interface ids {
  use acme:base/core@0.1.0.{stamp};
  record trace-id {
    value: u64,
    at: stamp,
  }
}
";
    let evolved_v0 = r"package acme:evolved@0.1.0;
interface meta {
  use acme:const/ids@0.1.0.{trace-id};
  record envelope {
    label: string,
    origin: trace-id,
  }
}
";
    let evolved_v1 = r"package acme:evolved@0.2.0;
interface meta {
  use acme:const/ids@0.1.0.{trace-id};
  record envelope {
    label: string,
    origin: trace-id,
    revision: u32,
  }
}
";
    let wrapper_v0 = r"package acme:wrapper@0.1.0;
interface box {
  use acme:evolved/meta@0.1.0.{envelope};
  record wrapped {
    inner: envelope,
  }
}
";
    let wrapper_v1 = r"package acme:wrapper@0.1.0;
interface box {
  use acme:evolved/meta@0.2.0.{envelope};
  record wrapped {
    inner: envelope,
  }
}
";
    let first_alias_wit = r"package a:b@0.1.0;
interface c-d {
  record first {
    n: u32,
  }
}
";
    let second_alias_wit = r"package a:b-c@0.1.0;
interface d {
  record second {
    s: string,
  }
}
";

    for (ver_dir, pkg_ver, evolved_src, wrapper_src, world_block) in [
        ("v0.1", "0.1.0", evolved_v0, wrapper_v0, ""),
        ("v1.0", "1.0.0", evolved_v0, wrapper_v0, ""),
        (
            "v2.0",
            "2.0.0",
            evolved_v1,
            wrapper_v1,
            "\nworld runner {\n  import api;\n}\n",
        ),
    ] {
        let dir = wit_root.join(ver_dir);
        fs::create_dir_all(dir.join("deps/acme-base")).unwrap();
        fs::create_dir_all(dir.join("deps/acme-const")).unwrap();
        fs::create_dir_all(dir.join("deps/acme-evolved")).unwrap();
        fs::create_dir_all(dir.join("deps/acme-wrapper")).unwrap();
        fs::create_dir_all(dir.join("deps/a-b")).unwrap();
        fs::create_dir_all(dir.join("deps/a-bc")).unwrap();
        fs::write(dir.join("deps/acme-base/base.wit"), base_wit).unwrap();
        fs::write(dir.join("deps/acme-const/const.wit"), const_wit).unwrap();
        fs::write(dir.join("deps/acme-evolved/evolved.wit"), evolved_src).unwrap();
        fs::write(dir.join("deps/acme-wrapper/wrapper.wit"), wrapper_src).unwrap();
        fs::write(dir.join("deps/a-b/ab.wit"), first_alias_wit).unwrap();
        fs::write(dir.join("deps/a-bc/abc.wit"), second_alias_wit).unwrap();
        let dep_ver = if ver_dir == "v2.0" { "0.2.0" } else { "0.1.0" };
        fs::write(
            dir.join("contract.wit"),
            format!(
                "package acme:multi@{pkg_ver};\n\
                 interface api {{\n\
                   use acme:const/ids@0.1.0.{{trace-id}};\n\
                   use acme:evolved/meta@{dep_ver}.{{envelope}};\n\
                   use acme:wrapper/box@0.1.0.{{wrapped}};\n\
                   use a:b/c-d@0.1.0.{{first}};\n\
                   use a:b-c/d@0.1.0.{{second}};\n\
                   record msg {{\n\
                     trace: trace-id,\n\
                     env: envelope,\n\
                     wrap: wrapped,\n\
                     one: first,\n\
                     two: second,\n\
                   }}\n\
                 }}\n{world_block}"
            ),
        )
        .unwrap();
        lock_version_dir(&dir, None);
    }
}

#[allow(clippy::too_many_lines)]
#[test]
fn all_constructs_golden_determinism_pep561_and_live_python3_runtime() {
    let base = scratch("all-constructs");
    let wit_root = base.join("wit");
    write_all_constructs_contract(&wit_root);

    let planned = python::plan(&wit_root).unwrap();
    let planned_again = python::plan_with_options(&wit_root, None, &PythonOptions::new()).unwrap();
    assert_eq!(
        planned, planned_again,
        "Python emission must be deterministic"
    );
    assert!(planned.is_nested());
    assert!(planned.markers().contains("py.typed"));
    assert_eq!(
        planned.files().get("py.typed").map(String::as_str),
        Some("")
    );

    let expected_header = HeaderStyle::HashComment.format_header("py", "acme", "showcase");
    assert_eq!(planned.header(), expected_header);

    for (path, content) in planned.files() {
        assert!(!content.contains('\r'), "{path} must be LF-only");
        if path == "py.typed" {
            assert!(content.is_empty(), "py.typed must be empty");
            continue;
        }
        assert!(
            content.starts_with(&format!("{expected_header}\n")),
            "{path} missing ownership header:\n{content}"
        );
        assert!(
            content.ends_with('\n') && !content.ends_with("\n\n"),
            "{path} must end with a single LF"
        );
        for (line_no, line) in content.lines().enumerate() {
            assert!(
                !line.ends_with(' ') && !line.ends_with('\t'),
                "{path}:{} has trailing whitespace: {line:?}",
                line_no + 1
            );
        }
        assert!(
            content.contains("from __future__ import annotations\n"),
            "{path} missing future annotations import"
        );
    }

    // Spot-check key Python mappings offline.
    let types_py = &planned.files()["v0_1/types.py"];
    assert!(types_py.contains("class Color(StrEnum):"), "{types_py}");
    assert!(
        types_py.contains("IN_PROGRESS = \"in-progress\""),
        "{types_py}"
    );
    assert!(types_py.contains("a_u64: int"), "{types_py}");
    assert!(types_py.contains("a_f64: float"), "{types_py}");
    assert!(types_py.contains("a_char: str"), "{types_py}");
    assert!(types_py.contains("raw_bytes: bytes"), "{types_py}");
    assert!(types_py.contains("alias_bytes: bytes"), "{types_py}");
    assert!(types_py.contains("dep_alias_bytes: bytes"), "{types_py}");
    assert!(types_py.contains("fixed_raw: bytes"), "{types_py}");
    assert!(
        types_py.contains("fixed_nums: tuple[int, ...]"),
        "{types_py}"
    );
    assert!(types_py.contains("opt_u8: int | None"), "{types_py}");
    assert!(
        types_py.contains("nested_opt: Option[int | None]"),
        "{types_py}"
    );
    assert!(
        types_py.contains("triple_opt: Option[Option[int | None]]"),
        "{types_py}"
    );
    assert!(
        types_py.contains("aliased_opt: Option[MaybeU16]"),
        "{types_py}"
    );
    assert!(types_py.contains("dep_opt: Option[DepMaybe]"), "{types_py}");
    assert!(
        types_py.contains("DepMaybe: TypeAlias = _d_acme_shared_common.DepMaybe"),
        "{types_py}"
    );
    assert!(
        types_py.contains("str_map: Mapping[str, int]"),
        "{types_py}"
    );
    assert!(
        types_py.contains("both_res: Ok[int] | Err[str]"),
        "{types_py}"
    );
    assert!(
        types_py.contains("only_ok: Ok[int] | Err[None]"),
        "{types_py}"
    );
    assert!(
        types_py.contains("only_err: Ok[None] | Err[Color]"),
        "{types_py}"
    );
    assert!(
        types_py.contains("bare: Ok[None] | Err[None]"),
        "{types_py}"
    );
    assert!(types_py.contains("class FooBarBaz:"), "{types_py}");
    assert!(types_py.contains("class Foo_BarBaz:"), "{types_py}");
    assert!(
        types_py.contains("Foo: TypeAlias = Foo_BarBaz | FooOther"),
        "{types_py}"
    );
    assert!(
        types_py.contains("import builtins as _wf_builtins"),
        "{types_py}"
    );
    assert!(types_py.contains("self_: _wf_builtins.int"), "{types_py}");
    assert!(types_py.contains("bool: _wf_builtins.bool"), "{types_py}");
    assert!(types_py.contains("class_: _wf_builtins.str"), "{types_py}");
    assert!(types_py.contains("read: bool = False"), "{types_py}");
    assert!(types_py.contains("self_: bool = False"), "{types_py}");
    assert!(types_py.contains("mro_: bool = False"), "{types_py}");
    assert!(types_py.contains("mro_: int"), "{types_py}");
    // Block-comment doc on `FixedLengthList` strips ` * ` gutters cleanly.
    assert!(
        types_py.contains("# Fixed 4-byte token header.\n#\n# Fixed-length list of exactly 4 elements.\nFixedBytes: TypeAlias = bytes"),
        "{types_py}"
    );
    assert!(!types_py.contains("#  * Fixed 4-byte"), "{types_py}");
    // Topological alias order: Color class -> AliasChainMid -> AliasChainTop.
    let color_pos = types_py.find("class Color(StrEnum):").unwrap();
    let mid_pos = types_py.find("AliasChainMid: TypeAlias =").unwrap();
    let top_pos = types_py.find("AliasChainTop: TypeAlias =").unwrap();
    assert!(color_pos < mid_pos && mid_pos < top_pos, "{types_py}");

    // Shadowed stdlib/typing/witforge symbols are aliased cleanly.
    let shadow_py = &planned.files()["v0_1/shadow.py"];
    assert!(shadow_py.contains("Mapping as _WfMapping"), "{shadow_py}");
    assert!(shadow_py.contains("StrEnum as _WfStrEnum"), "{shadow_py}");
    assert!(shadow_py.contains("Protocol as _WfProtocol"), "{shadow_py}");
    assert!(
        shadow_py.contains("TypeAlias as _WfTypeAlias"),
        "{shadow_py}"
    );
    assert!(
        shadow_py.contains("Err as _WfErr, Ok as _WfOk, Option as _WfOption"),
        "{shadow_py}"
    );
    assert!(shadow_py.contains("class None_:"), "{shadow_py}");
    assert!(shadow_py.contains("class True_:"), "{shadow_py}");
    assert!(shadow_py.contains("class False_:"), "{shadow_py}");

    // Escaped module stems for Python keywords and Windows reserved stems.
    assert!(planned.files().contains_key("v0_1/class_.py"));
    assert!(planned.files().contains_key("v0_1/from_.py"));
    assert!(planned.files().contains_key("v0_1/type_.py"));
    assert!(planned.files().contains_key("v0_1/con_.py"));
    assert!(planned.files().contains_key("v0_1/nul_.py"));
    assert!(
        planned
            .files()
            .contains_key("witforge_deps/acme_shared/class_.py")
    );
    assert!(
        planned
            .files()
            .contains_key("witforge_deps/acme_shared/con_.py")
    );
    let worlds_py = &planned.files()["worlds.py"];
    assert!(worlds_py.contains("class WorldMeta:"), "{worlds_py}");
    assert!(
        worlds_py.contains(
            "World `plugin` stability: Stable since WIT version 0.1.0. Deprecated in WIT version 0.1.0."
        ),
        "{worlds_py}"
    );
    assert!(
        worlds_py.contains(
            "World `plugin` import `acme:shared/common` stability: Stable since WIT version 0.1.0."
        ),
        "{worlds_py}"
    );
    assert!(
        worlds_py
            .contains("World `plugin` export `api` stability: Unstable WIT feature `beta-export`."),
        "{worlds_py}"
    );

    // Verify `emit_version` matches `plan`'s `v0_1/` files byte-for-byte.
    let single_model = model::build(&load(&wit_root.join("v0.1")).unwrap()).unwrap();
    let single_files = python::emit_version(&single_model).unwrap();
    for (file_name, content) in &single_files {
        assert_eq!(
            planned.files().get(&format!("v0_1/{file_name}")),
            Some(content),
            "emit_version mismatch on {file_name}"
        );
    }

    // Write to disk & verify live Python 3.11+ import, compileall, typing.get_type_hints,
    // frozen/slotted dataclasses, StrEnum, Protocol, nested Option, Result, and WORLDS.
    let pkg_dir = base.join("pkg_showcase");
    python::generate(&wit_root, &pkg_dir).unwrap();
    assert!(python::check(&wit_root, &pkg_dir).unwrap().is_empty());

    if let Some(py) = find_python311() {
        let script = r#"
import compileall
import dataclasses
import enum
import importlib
import inspect
import pkgutil
import sys
import typing

sys.path.insert(0, sys.argv[1])
assert compileall.compile_dir(sys.argv[2], quiet=1, force=True)

pkg = importlib.import_module("pkg_showcase")
# Walk and import every single module in the generated tree.
imported = [pkg]
for mod_info in pkgutil.walk_packages(pkg.__path__, pkg.__name__ + "."):
    imported.append(importlib.import_module(mod_info.name))
assert len(imported) >= 12, f"expected >= 12 modules, got {len(imported)}"

# Resolve typing.get_type_hints on every class and Protocol method in every module!
for mod in imported:
    for name, obj in inspect.getmembers(mod, inspect.isclass):
        if obj.__module__ != mod.__name__:
            continue
        if dataclasses.is_dataclass(obj):
            hints = typing.get_type_hints(obj)
            assert "__slots__" in obj.__dict__, f"{mod.__name__}.{name} missing __slots__"
            assert obj.__dataclass_params__.frozen, f"{mod.__name__}.{name} not frozen"
            if name not in ("NoneValue", "ChoiceUnitCase", "FooOther"):
                assert hints or len(dataclasses.fields(obj)) == 0
        elif getattr(obj, "_is_protocol", False):
            for mname, meth in inspect.getmembers(obj, inspect.isfunction):
                if mname.startswith("_"):
                    continue
                hints = typing.get_type_hints(meth)
                assert "return" in hints, f"{mod.__name__}.{name}.{mname} missing return hint"

t = pkg.v0_1.types
# StrEnum behavior and escaped docstring round-trip.
assert issubclass(t.Color, enum.StrEnum)
assert t.Color.RED == "red"
assert str(t.Color.IN_PROGRESS) == "in-progress"
assert '"""triple quotes"""' in (t.Color.__doc__ or "")
assert "Deprecated" in (t.Color.__doc__ or "")

# Nested Option distinguishes NONE, Some(None), Some(NONE), and Some(Some(5)).
assert pkg.NONE == pkg.NoneValue()
assert pkg.NONE != pkg.Some(None)
assert pkg.Some(None) != pkg.Some(5)
assert pkg.Some(pkg.NONE) != pkg.NONE
assert pkg.Some(pkg.Some(None)) != pkg.Some(pkg.NONE)

# Frozen & slotted immutability check.
ok_val = pkg.Ok(42)
assert not hasattr(ok_val, "__dict__")
try:
    ok_val.value = 99
    raise AssertionError("Ok must be frozen")
except dataclasses.FrozenInstanceError:
    pass

# Flags defaults and keyword overrides.
p_default = t.Perms()
assert p_default.read is False and p_default.self_ is False and p_default.mro_ is False
assert isinstance(t.Perms.mro(), list)
p_custom = t.Perms(read=True, class_=True, self_=True, mro_=True)
assert p_custom.read is True and p_custom.write is False and p_custom.class_ is True and p_custom.mro_ is True

# `mro` escaped cleanly on records (first and sole field) and Protocol without breaking `.mro()`.
mf = t.MroFirst(mro_=1, name="ok")
assert mf.mro_ == 1 and mf.name == "ok" and isinstance(t.MroFirst.mro(), list)
mo = t.MroOnly(mro_=2)
assert mo.mro_ == 2 and isinstance(t.MroOnly.mro(), list)
try:
    t.MroOnly()  # must require `mro_`, never default to `type.mro`
    raise AssertionError("MroOnly() must require mro_")
except TypeError:
    pass
assert isinstance(pkg.v0_1.api.Api.mro(), list)
assert hasattr(pkg.v0_1.api.Api, "mro_")

# Builtin-named & keyword fields record.
bf = t.BuiltinFields(
    bool=True, int=1, float=1.5, str="hi", bytes=b"ab", tuple=(1, 2),
    self_=7, mro_=8, class_="c", from_=False, match_=3, type_=4,
)
assert bf.self_ == 7 and bf.mro_ == 8 and bf.bytes == b"ab" and bf.tuple == (1, 2)

# Variant disambiguation + structural pattern matching.
v_collided = t.Foo_BarBaz(9)
rec_collided = t.FooBarBaz(id=10)
assert v_collided.value == 9 and rec_collided.id == 10
match v_collided:
    case t.Foo_BarBaz(n):
        assert n == 9
    case _:
        raise AssertionError("match failed")

# Result pattern matching across all 4 shapes.
for r, expected in [
    (pkg.Ok(1), 1),
    (pkg.Ok(None), None),
    (pkg.Err("bad"), "bad"),
    (pkg.Err(None), None),
]:
    match r:
        case pkg.Ok(v):
            assert v == expected
        case pkg.Err(e):
            assert e == expected

# Shadowed symbols module works cleanly alongside real stdlib/witforge types.
sh = pkg.v0_1.shadow
sh_hints = typing.get_type_hints(sh.Carrier)
assert sh_hints["local_opt"] is sh.Option
assert sh_hints["local_ok"] is sh.Ok
assert sh_hints["local_err"] is sh.Err
assert sh_hints["local_none"] is sh.None_
assert sh_hints["local_true"] is sh.True_
assert sh_hints["local_false"] is sh.False_
assert sh_hints["real_enum"] is sh.Tone
assert issubclass(sh.Tone, enum.StrEnum)

# WORLDS read-only MappingProxyType verification.
assert "v0_1" in pkg.WORLDS
assert len(pkg.WORLDS["v0_1"]) == 1
meta = pkg.WORLDS["v0_1"][0]
assert isinstance(meta, pkg.WorldMeta)
assert meta.name == "plugin"
assert "acme:shared/common" in meta.imports
assert "types" in meta.imports
assert meta.exports == ("api",)
try:
    pkg.WORLDS["v0_1"] = ()
    raise AssertionError("WORLDS must be immutable")
except TypeError:
    pass
"#;
        let output = Command::new(&py)
            .args(["-c", script])
            .arg(&base)
            .arg(&pkg_dir)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "live python3 showcase check failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fs::remove_dir_all(&base).unwrap();
}

#[allow(clippy::too_many_lines)]
#[test]
fn multi_major_dep_layout_canonical_owner_and_live_python3_identity() {
    let base = scratch("multi-major");
    let wit_root = base.join("wit");
    write_three_major_dep_layout_contract(&wit_root);

    let planned = python::plan(&wit_root).unwrap();
    // Shared deps sit directly under `witforge_deps/<seg>/`.
    assert!(
        planned
            .files()
            .contains_key("witforge_deps/acme_base/core.py")
    );
    assert!(
        planned
            .files()
            .contains_key("witforge_deps/acme_const/ids.py")
    );
    // Scoped deps sit under `witforge_deps/v<major>/<seg>/`.
    assert!(
        planned
            .files()
            .contains_key("witforge_deps/v0/acme_evolved/meta.py")
    );
    assert!(
        planned
            .files()
            .contains_key("witforge_deps/v1/acme_evolved/meta.py")
    );
    assert!(
        planned
            .files()
            .contains_key("witforge_deps/v2/acme_evolved/meta.py")
    );

    // v1 canonical owner is v0, so v1 re-exports from `...v0.acme_evolved.meta`.
    let v1_evolved = &planned.files()["witforge_deps/v1/acme_evolved/meta.py"];
    assert!(
        v1_evolved.contains("from ...v0.acme_evolved.meta import ("),
        "{v1_evolved}"
    );
    assert!(v1_evolved.contains("Envelope as Envelope,"), "{v1_evolved}");
    // v2 defines its own upgraded `revision: int` field.
    let v2_evolved = &planned.files()["witforge_deps/v2/acme_evolved/meta.py"];
    assert!(v2_evolved.contains("revision: int"), "{v2_evolved}");
    // `a:b/c-d` and `a:b-c/d` deduplicate import aliases cleanly (`_d_a_b_c_d` and `_d_a_b_c_d_2`).
    let v0_api = &planned.files()["v0_1/api.py"];
    assert!(v0_api.contains("_d_a_b_c_d"), "{v0_api}");
    assert!(v0_api.contains("_d_a_b_c_d_2"), "{v0_api}");

    let pkg_dir = base.join("pkg_multi");
    python::generate(&wit_root, &pkg_dir).unwrap();

    if let Some(py) = find_python311() {
        let script = r#"
import compileall
import dataclasses
import importlib
import inspect
import pkgutil
import sys
import typing

sys.path.insert(0, sys.argv[1])
assert compileall.compile_dir(sys.argv[2], quiet=1, force=True)

pkg = importlib.import_module("pkg_multi")
imported = [pkg]
for mod_info in pkgutil.walk_packages(pkg.__path__, pkg.__name__ + "."):
    imported.append(importlib.import_module(mod_info.name))

for mod in imported:
    for name, obj in inspect.getmembers(mod, inspect.isclass):
        if obj.__module__ == mod.__name__ and dataclasses.is_dataclass(obj):
            typing.get_type_hints(obj)

# Canonical-owner re-export preserves exact Python class identity (`is`) between v0 and v1!
assert pkg.witforge_deps.v1.acme_evolved.meta.Envelope is pkg.witforge_deps.v0.acme_evolved.meta.Envelope
assert pkg.witforge_deps.v1.acme_wrapper.box.Wrapped is pkg.witforge_deps.v0.acme_wrapper.box.Wrapped
# v2 upgraded `acme:evolved@0.2.0` is a distinct class with `revision`.
assert pkg.witforge_deps.v2.acme_evolved.meta.Envelope is not pkg.witforge_deps.v0.acme_evolved.meta.Envelope
v2_fields = {f.name for f in dataclasses.fields(pkg.witforge_deps.v2.acme_evolved.meta.Envelope)}
assert "revision" in v2_fields

# Construct v0, v1, and v2 messages sharing v0/v1 canonical Envelope instance!
stamp = pkg.witforge_deps.acme_base.core.Stamp(epoch=1234567890123456789)
tid = pkg.witforge_deps.acme_const.ids.TraceId(value=42, at=stamp)
env0 = pkg.witforge_deps.v0.acme_evolved.meta.Envelope(label="v0", origin=tid)
wrap1 = pkg.witforge_deps.v1.acme_wrapper.box.Wrapped(inner=env0)
msg1 = pkg.v1_0.api.Msg(
    trace=tid,
    env=env0,
    wrap=wrap1,
    one=pkg.witforge_deps.a_b.c_d.First(n=1),
    two=pkg.witforge_deps.a_b_c.d.Second(s="two"),
)
assert msg1.env is env0
assert pkg.WORLDS["v0_1"] == ()
assert pkg.WORLDS["v1_0"] == ()
assert len(pkg.WORLDS["v2_0"]) == 1 and pkg.WORLDS["v2_0"][0].name == "runner"
"#;
        let output = Command::new(&py)
            .args(["-c", script])
            .arg(&base)
            .arg(&pkg_dir)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "live python3 multi-major check failed:\nstdout:\n{}\nstderr:\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fs::remove_dir_all(&base).unwrap();
}

#[allow(clippy::too_many_lines)]
#[test]
fn ownership_drift_marker_pruning_retired_and_conformance_contract() {
    let base = scratch("ownership-and-conformance");
    let wit_root = fixture("versions/wit");
    let out = base.join("py_out");

    python::generate(&wit_root, &out).unwrap();
    assert!(python::check(&wit_root, &out).unwrap().is_empty());

    // Mutating an owned file reports Changed; mutating `py.typed` marker reports Unowned and
    // is refused on write (never overwrites non-empty hand-written marker bytes).
    let init_file = out.join("__init__.py");
    let orig = fs::read_to_string(&init_file).unwrap();
    fs::write(&init_file, format!("{orig}# hand edit\n")).unwrap();
    let findings = python::check(&wit_root, &out).unwrap();
    assert_eq!(findings.len(), 1);
    assert!(matches!(
        findings[0].status,
        GeneratedFileStatus::Changed { .. }
    ));
    fs::write(&init_file, &orig).unwrap();

    fs::write(out.join("py.typed"), "custom marker\n").unwrap();
    let marker_findings = python::check(&wit_root, &out).unwrap();
    assert_eq!(marker_findings.len(), 1);
    assert_eq!(marker_findings[0].status, GeneratedFileStatus::Unowned);
    assert!(python::generate(&wit_root, &out).is_err());
    fs::write(out.join("py.typed"), "").unwrap();

    // Stale owned file in nested directory is pruned along with its empty directory while
    // preserving unrelated unowned files.
    let header = HeaderStyle::HashComment.format_header("py", "example", "evolve");
    let stale_dir = out.join("v0_0");
    fs::create_dir_all(&stale_dir).unwrap();
    fs::write(stale_dir.join("stale.py"), format!("{header}\n")).unwrap();
    fs::write(out.join("README.txt"), "user note\n").unwrap();
    let extra_findings = python::check(&wit_root, &out).unwrap();
    assert_eq!(extra_findings.len(), 1);
    assert_eq!(extra_findings[0].status, GeneratedFileStatus::Extra);
    python::generate(&wit_root, &out).unwrap();
    assert!(!stale_dir.exists(), "empty stale dir must be pruned");
    assert_eq!(
        fs::read_to_string(out.join("README.txt")).unwrap(),
        "user note\n"
    );

    // Foreign ownership header (Rust or TypeScript) is refused on both write and check.
    let foreign_dir = base.join("foreign_out");
    fs::create_dir_all(&foreign_dir).unwrap();
    fs::write(
        foreign_dir.join("mod.rs"),
        "// @generated by witforge (rust, example:evolve); do not edit.\n",
    )
    .unwrap();
    let err = python::generate(&wit_root, &foreign_dir).unwrap_err();
    assert!(!is_contract_error(&err), "{err:#}");
    assert!(format!("{err:#}").contains("generator `rust`"), "{err:#}");

    // Retired versions are excluded via `plan_with_released`.
    let rel_base = base.join("retired_wit");
    fs::create_dir_all(&rel_base).unwrap();
    for ver in ["v0.1", "v0.2"] {
        let dst = rel_base.join(ver);
        fs::create_dir_all(&dst).unwrap();
        for entry in fs::read_dir(wit_root.join(ver)).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_file() {
                fs::copy(entry.path(), dst.join(entry.file_name())).unwrap();
            }
        }
    }
    let manifest = rel_base.join("released");
    fs::write(&manifest, "v0.1 retired\nv0.2\n").unwrap();
    let live_only = python::plan_with_released(&rel_base, Some(&manifest)).unwrap();
    assert!(!live_only.files().contains_key("v0_1/__init__.py"));
    assert!(live_only.files().contains_key("v0_2/__init__.py"));

    // Standalone `emit_version` works on a single model.
    let single_model = model::build(&load(&wit_root.join("v0.1")).unwrap()).unwrap();
    assert_eq!(python::version_dir_name(&single_model), "v0_1");
    let single_files = python::emit_version(&single_model).unwrap();
    assert!(single_files.contains_key("__init__.py"));

    // Emit the canonical W11-034 conformance contract and verify live Python import + type hints.
    let conf_dir = base.join("conf_bundle");
    conformance::generate(&conf_dir, &conformance::ConformanceOptions::new()).unwrap();
    let conf_py = base.join("pkg_conf");
    python::generate(&conf_dir.join("wit"), &conf_py).unwrap();
    assert!(
        python::check(&conf_dir.join("wit"), &conf_py)
            .unwrap()
            .is_empty()
    );

    if let Some(py) = find_python311() {
        let script = r#"
import compileall, dataclasses, importlib, inspect, pkgutil, sys, typing
sys.path.insert(0, sys.argv[1])
assert compileall.compile_dir(sys.argv[2], quiet=1, force=True)
pkg = importlib.import_module("pkg_conf")
for mod_info in pkgutil.walk_packages(pkg.__path__, pkg.__name__ + "."):
    mod = importlib.import_module(mod_info.name)
    for _, obj in inspect.getmembers(mod, inspect.isclass):
        if obj.__module__ == mod.__name__ and dataclasses.is_dataclass(obj):
            typing.get_type_hints(obj)
"#;
        let output = Command::new(&py)
            .args(["-c", script])
            .arg(&base)
            .arg(&conf_py)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "live python3 conformance check failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fs::remove_dir_all(&base).unwrap();
}

#[allow(clippy::too_many_lines)]
#[test]
fn programmatic_edge_cases_never_multiline_stability_collisions_and_canonical_reexport_stems() {
    use witforge::model::{
        Case, Field, Function, Interface, Model, Package, Stability, TypeDef, TypeDefKind,
        TypeExpr, Version,
    };

    // 1. Programmatic `Model` with empty `Variant` (emitting `Never` + `_WfNever` shadowing),
    //    empty `Record`/`Enum`/`Flags`, and multiline/`\r` `Stability` notes -> LF-only valid Python.
    let multiline_stability = Stability::Deprecated {
        since: Some(Version::new(0, 1, 0)),
        feature: None,
        deprecated: Version::new(0, 2, 0),
        note: "Deprecated in WIT version 0.2.0.\r\nUse replacement instead.".to_string(),
    };
    let prog_model = Model::new(
        Package::new(
            "acme".into(),
            "prog".into(),
            Version::new(0, 2, 0),
            String::new(),
        ),
        vec![Interface::new(
            "edge".into(),
            String::new(),
            Stability::Unknown,
            vec![
                TypeDef::new(
                    "never".into(),
                    String::new(),
                    Stability::Unknown,
                    TypeDefKind::Record(Vec::new()),
                ),
                TypeDef::new(
                    "void-variant".into(),
                    String::new(),
                    multiline_stability.clone(),
                    TypeDefKind::Variant(Vec::new()),
                ),
                TypeDef::new(
                    "empty-enum".into(),
                    String::new(),
                    Stability::Unknown,
                    TypeDefKind::Enum(Vec::new()),
                ),
                TypeDef::new(
                    "empty-flags".into(),
                    String::new(),
                    Stability::Unknown,
                    TypeDefKind::Flags(Vec::new()),
                ),
                TypeDef::new(
                    "multiline-alias".into(),
                    "\r\n * Block doc.\r\n ".into(),
                    multiline_stability,
                    TypeDefKind::FixedLengthList(TypeExpr::U16, 2),
                ),
            ],
            Vec::new(),
        )],
        Vec::new(),
        Vec::new(),
    );

    let files = python::emit_version(&prog_model).unwrap();
    let edge_py = &files["edge.py"];
    assert!(!edge_py.contains('\r'), "must be LF-only:\n{edge_py}");
    assert!(edge_py.contains("Never as _WfNever"), "{edge_py}");
    assert!(
        edge_py.contains("VoidVariant: TypeAlias = _WfNever"),
        "{edge_py}"
    );
    assert!(
        edge_py.contains("# Use replacement instead.\n"),
        "{edge_py}"
    );
    assert!(
        edge_py.contains("# Block doc.\n#\n# Fixed-length list of exactly 2 elements."),
        "{edge_py}"
    );
    assert!(!edge_py.contains("#  * Block doc."), "{edge_py}");

    let base = scratch("prog-edge");
    if let Some(py) = find_python311() {
        let pkg = base.join("prog_pkg");
        fs::create_dir_all(&pkg).unwrap();
        fs::write(pkg.join("__init__.py"), "").unwrap();
        fs::write(pkg.join("edge.py"), edge_py).unwrap();
        let output = Command::new(&py)
            .args([
                "-c",
                "import compileall, importlib, sys; sys.path.insert(0, sys.argv[1]); \
                 assert compileall.compile_dir(sys.argv[2], quiet=1, force=True); \
                 m = importlib.import_module('prog_pkg.edge'); \
                 assert m.Never() == m.Never()",
            ])
            .arg(&base)
            .arg(&pkg)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "live python3 programmatic check failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    // 2. ContractError refusals for interface-vs-type name collision and duplicate enum/variant cases.
    let colliding = Model::new(
        Package::new(
            "acme".into(),
            "bad".into(),
            Version::new(0, 1, 0),
            String::new(),
        ),
        vec![Interface::new(
            "service".into(),
            String::new(),
            Stability::Unknown,
            vec![TypeDef::new(
                "service".into(),
                String::new(),
                Stability::Unknown,
                TypeDefKind::Record(vec![Field::new("id".into(), String::new(), TypeExpr::U32)]),
            )],
            vec![Function::new(
                "ping".into(),
                String::new(),
                Stability::Unknown,
                Vec::new(),
                None,
            )],
        )],
        Vec::new(),
        Vec::new(),
    );
    let err = python::emit_version(&colliding).unwrap_err();
    assert!(is_contract_error(&err), "{err:#}");

    let dup_variant = Model::new(
        Package::new(
            "acme".into(),
            "bad".into(),
            Version::new(0, 1, 0),
            String::new(),
        ),
        vec![Interface::new(
            "types".into(),
            String::new(),
            Stability::Unknown,
            vec![TypeDef::new(
                "v".into(),
                String::new(),
                Stability::Unknown,
                TypeDefKind::Variant(vec![
                    Case::new("a-b".into(), String::new(), None),
                    Case::new("A-B".into(), String::new(), None),
                ]),
            )],
            Vec::new(),
        )],
        Vec::new(),
        Vec::new(),
    );
    let dup_err = python::emit_version(&dup_variant).unwrap_err();
    assert!(is_contract_error(&dup_err), "{dup_err:#}");

    // 3. Canonical-owner re-export of keyword-stem (`class_`), Windows-reserved (`con_`),
    //    Protocol-bearing, and empty dependency interfaces across majors.
    let wit_root = base.join("canon_wit");
    let dep_v0 = r"package acme:tricky@0.1.0;
interface %class {
  record item { id: u32 }
  ping: func(x: item) -> item;
}
interface con {
  record link { ok: bool }
}
interface empty {}
";
    let dep_v2 = r"package acme:tricky@0.2.0;
interface %class {
  record item { id: u64 }
  ping: func(x: item) -> item;
}
interface con {
  record link { ok: bool, code: u8 }
}
interface empty {}
";
    for (ver_dir, pkg_ver, dep_src, dep_ver) in [
        ("v0.1", "0.1.0", dep_v0, "0.1.0"),
        ("v1.0", "1.0.0", dep_v0, "0.1.0"),
        ("v2.0", "2.0.0", dep_v2, "0.2.0"),
    ] {
        let dir = wit_root.join(ver_dir);
        fs::create_dir_all(dir.join("deps/tricky")).unwrap();
        fs::write(dir.join("deps/tricky/tricky.wit"), dep_src).unwrap();
        fs::write(
            dir.join("contract.wit"),
            format!(
                "package acme:app@{pkg_ver};\n\
                 interface api {{\n\
                   use acme:tricky/%class@{dep_ver}.{{item}};\n\
                   use acme:tricky/con@{dep_ver}.{{link}};\n\
                   record box {{ i: item, l: link }}\n\
                 }}\n"
            ),
        )
        .unwrap();
        lock_version_dir(&dir, None);
    }

    let canon_pkg = base.join("canon_pkg");
    python::generate(&wit_root, &canon_pkg).unwrap();
    assert!(python::check(&wit_root, &canon_pkg).unwrap().is_empty());

    if let Some(py) = find_python311() {
        let output = Command::new(&py)
            .args([
                "-c",
                "import compileall, importlib, sys; sys.path.insert(0, sys.argv[1]); \
                 assert compileall.compile_dir(sys.argv[2], quiet=1, force=True); \
                 p = importlib.import_module('canon_pkg'); \
                 assert p.witforge_deps.v1.acme_tricky.class_.Item is p.witforge_deps.v0.acme_tricky.class_.Item; \
                 assert p.witforge_deps.v1.acme_tricky.class_.Class is p.witforge_deps.v0.acme_tricky.class_.Class; \
                 assert p.witforge_deps.v1.acme_tricky.con_.Link is p.witforge_deps.v0.acme_tricky.con_.Link; \
                 assert p.witforge_deps.v1.acme_tricky.empty.__all__ == (); \
                 assert p.witforge_deps.v2.acme_tricky.class_.Item is not p.witforge_deps.v0.acme_tricky.class_.Item",
            ])
            .arg(&base)
            .arg(&canon_pkg)
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "live python3 canonical tricky dep check failed:\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fs::remove_dir_all(&base).unwrap();
}

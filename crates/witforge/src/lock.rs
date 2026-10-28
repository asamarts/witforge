//! The wire lockfile: stable protobuf numbers for a contract whose source has none.
//!
//! WIT records, variants, enums, and flags are positional and carry no field numbers, but the
//! protobuf projection needs a number per member that never changes. The lockfile maps every
//! numbered member to a number, unique within its parent, and is append-only.
//!
//! The v2 text format is one entry per line, sorted by key:
//! `key = number kind [type] [reserved]`. Lines after the first header line starting with `#`
//! are comments, and blank lines are ignored.

use std::collections::BTreeMap;
use std::fmt::{self, Write as _};
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result, bail};

use crate::model::{Member, MemberKind};

/// The exact first line of every v2 lockfile.
pub const HEADER: &str = "# witforge wire lockfile v2";

/// Protobuf reserves field numbers 19000 to 19999 for its own implementation.
const PROTOBUF_RESERVED: std::ops::RangeInclusive<u32> = 19_000..=19_999;
/// The largest valid protobuf field number.
const PROTOBUF_MAX: u32 = 536_870_911;

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A package name in a lockfile key: `namespace:name`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PackageName {
    /// The package namespace.
    pub namespace: String,
    /// The package name.
    pub name: String,
}

impl fmt::Display for PackageName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.namespace, self.name)
    }
}

/// A parsed, typed lockfile key.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum MemberKey {
    /// `package/interface/type.member`.
    TypeMember {
        /// The owning package.
        package: PackageName,
        /// The defining interface.
        interface: String,
        /// The defining type.
        type_name: String,
        /// The member within the type.
        member: String,
    },
    /// `package/interface.function`.
    Method {
        /// The owning package.
        package: PackageName,
        /// The defining interface.
        interface: String,
        /// The function.
        function: String,
    },
    /// `package/interface#function.parameter`.
    Parameter {
        /// The owning package.
        package: PackageName,
        /// The defining interface.
        interface: String,
        /// The function.
        function: String,
        /// The parameter within the function.
        parameter: String,
    },
}

impl MemberKey {
    /// Parse a lockfile key into its typed scope.
    ///
    /// # Errors
    ///
    /// Returns an error for an unknown shape, an empty component, or a component that is not a
    /// WIT identifier.
    pub fn parse(value: &str) -> Result<Self> {
        let (package, rest) = value
            .split_once('/')
            .with_context(|| format!("malformed key `{value}`: missing `/`"))?;
        let package = parse_package(package)?;

        if let Some((interface, tail)) = rest.split_once('#') {
            ensure_no_separator("interface", interface, '#')?;
            let (function, parameter) = tail
                .split_once('.')
                .with_context(|| format!("malformed parameter key `{value}`: missing `.`"))?;
            ensure_ident("function", function)?;
            ensure_ident("parameter", parameter)?;
            return Ok(Self::Parameter {
                package,
                interface: interface.to_string(),
                function: function.to_string(),
                parameter: parameter.to_string(),
            });
        }

        if let Some((interface, tail)) = rest.split_once('/') {
            let (type_name, member) = tail
                .split_once('.')
                .with_context(|| format!("malformed type-member key `{value}`: missing `.`"))?;
            ensure_ident("interface", interface)?;
            ensure_ident("type", type_name)?;
            ensure_ident("member", member)?;
            return Ok(Self::TypeMember {
                package,
                interface: interface.to_string(),
                type_name: type_name.to_string(),
                member: member.to_string(),
            });
        }

        let (interface, function) = rest
            .split_once('.')
            .with_context(|| format!("malformed method key `{value}`: missing `.`"))?;
        ensure_ident("interface", interface)?;
        ensure_ident("function", function)?;
        Ok(Self::Method {
            package,
            interface: interface.to_string(),
            function: function.to_string(),
        })
    }

    /// Build a typed key from a model member.
    ///
    /// # Errors
    ///
    /// Returns an error when the member's generated key is malformed.
    pub fn from_member(member: &Member) -> Result<Self> {
        Self::parse(&member.key()).with_context(|| {
            format!(
                "member `{}.{}` has an invalid key",
                member.parent, member.name
            )
        })
    }

    /// The parent whose number space this key belongs to.
    #[must_use]
    pub fn parent(&self) -> String {
        match self {
            Self::TypeMember {
                package,
                interface,
                type_name,
                ..
            } => format!("{package}/{interface}/{type_name}"),
            Self::Method {
                package, interface, ..
            } => format!("{package}/{interface}"),
            Self::Parameter {
                package,
                interface,
                function,
                ..
            } => format!("{package}/{interface}#{function}"),
        }
    }

    /// The package that owns this key.
    #[must_use]
    pub fn package(&self) -> &PackageName {
        match self {
            Self::TypeMember { package, .. }
            | Self::Method { package, .. }
            | Self::Parameter { package, .. } => package,
        }
    }

    /// The canonical lockfile key.
    #[must_use]
    pub fn key(&self) -> String {
        match self {
            Self::TypeMember {
                package,
                interface,
                type_name,
                member,
            } => format!("{package}/{interface}/{type_name}.{member}"),
            Self::Method {
                package,
                interface,
                function,
            } => format!("{package}/{interface}.{function}"),
            Self::Parameter {
                package,
                interface,
                function,
                parameter,
            } => format!("{package}/{interface}#{function}.{parameter}"),
        }
    }
}

impl fmt::Display for MemberKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.key())
    }
}

/// One lockfile entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    /// The wire number, unique within the member's parent.
    pub number: u32,
    /// The member kind.
    pub kind: MemberKind,
    /// The canonical type, when the kind has one.
    pub ty: Option<String>,
    /// Whether the member was removed (its number stays taken).
    pub reserved: bool,
}

/// A parsed lockfile.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WireLock {
    entries: BTreeMap<MemberKey, Entry>,
}

/// The classification baseline for a lock operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LockMode<'a> {
    /// Treat every existing entry as released; the default conservative mode.
    Default,
    /// Treat every existing entry as a draft; for a version with no previous baseline.
    First,
    /// Treat entries from `previous` as released and all others as drafts.
    Against(&'a WireLock),
}

/// What [`WireLock::reconcile`] changed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Changes {
    /// Members that were assigned a new number.
    pub added: Vec<(String, u32)>,
    /// Members that disappeared from the contract and are now reserved.
    pub reserved: Vec<(String, u32)>,
    /// Members of the contract whose key is reserved: they were removed and have come back
    /// under the same name. They stay reserved and have no number, so the contract must give
    /// them a new name.
    pub reintroduced: Vec<(String, u32)>,
    /// Draft entries that were dropped because they were removed or reserved.
    pub dropped: Vec<(String, u32)>,
    /// Live draft entries whose kind or canonical type was updated.
    pub updated: Vec<(String, MemberKind, Option<String>)>,
}

impl Changes {
    /// Whether the lockfile already matches the contract: nothing changed or was refused.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.added.is_empty()
            && self.reserved.is_empty()
            && self.reintroduced.is_empty()
            && self.dropped.is_empty()
            && self.updated.is_empty()
    }
}

impl WireLock {
    /// Parse the v2 lockfile text.
    ///
    /// # Errors
    ///
    /// Returns an error for a missing or unknown header, a malformed key or entry, an invalid
    /// number, a bad canonical type, a duplicate key, or two members of one parent sharing a
    /// number. A v1 lockfile is refused rather than migrated.
    pub fn parse(text: &str) -> Result<Self> {
        let mut lines = text.lines();
        let Some(first) = lines.next() else {
            bail!("empty lockfile: expected `{HEADER}`");
        };
        if first != HEADER {
            bail!("line 1: expected `{HEADER}`");
        }

        let mut entries = BTreeMap::new();
        for (index, raw) in lines.enumerate() {
            let line_no = index + 2;
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (key_text, rest) = line
                .split_once('=')
                .with_context(|| format!("line {line_no}: expected `key = number kind [type]`"))?;
            let key =
                MemberKey::parse(key_text.trim()).with_context(|| format!("line {line_no}"))?;
            let (number, kind, ty, reserved) = parse_entry(rest, line_no)?;
            if !valid_number(number) {
                bail!("line {line_no}: {number} is not a usable protobuf field number");
            }
            if entries
                .insert(
                    key.clone(),
                    Entry {
                        number,
                        kind,
                        ty,
                        reserved,
                    },
                )
                .is_some()
            {
                bail!("line {line_no}: duplicate key `{key}`");
            }
        }
        let lock = Self { entries };
        lock.validate()?;
        Ok(lock)
    }

    /// The entry for a member key, if any.
    #[must_use]
    pub fn get(&self, key: &str) -> Option<Entry> {
        MemberKey::parse(key)
            .ok()
            .and_then(|key| self.entries.get(&key).cloned())
    }

    /// Bring the lockfile up to date with `members` in the conservative default mode.
    ///
    /// # Errors
    ///
    /// Returns an error when a member key is malformed, a released kind/type changed, or a
    /// parent has used every usable protobuf field number.
    pub fn reconcile(&mut self, members: &[Member]) -> Result<Changes> {
        self.reconcile_with(members, LockMode::Default)
    }

    /// Bring the lockfile up to date with `members` under an explicit classification mode.
    ///
    /// The operation is transactional: a released kind/type error, malformed key, or exhausted
    /// number space leaves this lockfile unchanged.
    ///
    /// # Errors
    ///
    /// Returns an error when the baseline is inconsistent, a released entry would change,
    /// a member key is malformed, or a parent has used every usable protobuf field number.
    pub fn reconcile_with(&mut self, members: &[Member], mode: LockMode<'_>) -> Result<Changes> {
        let mut staged = self.clone();
        let changes = staged.reconcile_in_place(members, mode)?;
        *self = staged;
        Ok(changes)
    }

    fn validate_against(
        &self,
        previous: &WireLock,
        live: &BTreeMap<MemberKey, &Member>,
    ) -> Result<()> {
        if let Some(first) = previous.entries.keys().next() {
            let expected = first.package();
            for key in previous.entries.keys() {
                if key.package() != expected {
                    bail!(
                        "previous lockfile contains package `{}`, expected `{expected}`",
                        key.package()
                    );
                }
            }
            for key in self.entries.keys().chain(live.keys()) {
                if key.package() != expected {
                    bail!(
                        "previous lockfile package `{expected}` does not match `{}`",
                        key.package()
                    );
                }
            }
        }
        for (key, old) in &previous.entries {
            let Some(current) = self.entries.get(key) else {
                bail!("released entry `{key}` is missing from the current lockfile");
            };
            if current.number != old.number {
                bail!(
                    "released entry `{key}` was renumbered from {} to {}; numbers never change",
                    old.number,
                    current.number
                );
            }
            if current.kind != old.kind || current.ty != old.ty {
                bail!("released entry `{key}` changed kind or type");
            }
            if old.reserved && !current.reserved {
                bail!("released entry `{key}` was un-reserved; reserved numbers are never reused");
            }
        }
        Ok(())
    }

    fn reconcile_in_place(&mut self, members: &[Member], mode: LockMode<'_>) -> Result<Changes> {
        let live: BTreeMap<MemberKey, &Member> = members
            .iter()
            .map(|member| Ok((MemberKey::from_member(member)?, member)))
            .collect::<Result<_>>()?;
        let released: BTreeMap<MemberKey, Entry> = match mode {
            LockMode::Default => self.entries.clone(),
            LockMode::First => BTreeMap::new(),
            LockMode::Against(previous) => previous.entries.clone(),
        };

        if let LockMode::Against(previous) = mode {
            self.validate_against(previous, &live)?;
        }

        let mut changes = Changes::default();
        let mut dropped = Vec::new();
        for (key, entry) in &self.entries {
            if !released.contains_key(key) && (entry.reserved || !live.contains_key(key)) {
                dropped.push((key.clone(), entry.number));
            }
        }
        for (key, number) in &dropped {
            self.entries.remove(key);
            changes.dropped.push((key.key(), *number));
        }

        for (key, entry) in &mut self.entries {
            if released.contains_key(key) && !entry.reserved && !live.contains_key(key) {
                entry.reserved = true;
                changes.reserved.push((key.key(), entry.number));
            }
        }

        for member in members {
            let key = MemberKey::from_member(member)?;
            match self.entries.get(&key) {
                Some(entry) if entry.reserved => {
                    changes.reintroduced.push((key.key(), entry.number));
                    continue;
                }
                Some(entry)
                    if entry.kind != member.kind || entry.ty.as_deref() != member.ty.as_deref() =>
                {
                    if released.contains_key(&key) {
                        bail!(
                            "released entry `{key}` changed kind or type from `{} {}` to `{} {}`",
                            entry.kind,
                            entry.ty.as_deref().unwrap_or("-"),
                            member.kind,
                            member.ty.as_deref().unwrap_or("-")
                        );
                    }
                    let entry = self.entries.get_mut(&key).expect("entry exists");
                    entry.kind = member.kind;
                    entry.ty.clone_from(&member.ty);
                    changes
                        .updated
                        .push((key.key(), member.kind, member.ty.clone()));
                    continue;
                }
                Some(_) => continue,
                None => {}
            }
            let number = self.next_number(&key.parent())?;
            self.entries.insert(
                key.clone(),
                Entry {
                    number,
                    kind: member.kind,
                    ty: member.ty.clone(),
                    reserved: false,
                },
            );
            changes.added.push((key.key(), number));
        }
        Ok(changes)
    }

    /// Members of the contract that the lockfile does not number, live lockfile entries whose
    /// member no longer exists, and removed members that came back. All empty means the
    /// lockfile is current.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`WireLock::reconcile`].
    pub fn drift(&self, members: &[Member]) -> Result<Changes> {
        self.drift_with(members, LockMode::Default)
    }

    /// Like [`WireLock::drift`] with an explicit classification mode.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`WireLock::reconcile_with`].
    pub fn drift_with(&self, members: &[Member], mode: LockMode<'_>) -> Result<Changes> {
        let mut copy = self.clone();
        copy.reconcile_with(members, mode)
    }

    /// Render the lockfile text (sorted, stable, LF line endings).
    #[must_use]
    pub fn render(&self) -> String {
        let mut out = format!("{HEADER}\n");
        let mut keys: Vec<&MemberKey> = self.entries.keys().collect();
        keys.sort_by_key(|key| key.key());
        for key in keys {
            let entry = &self.entries[key];
            let _ = write!(out, "{} = {} {}", key.key(), entry.number, entry.kind);
            if let Some(ty) = &entry.ty {
                let _ = write!(out, " {ty}");
            }
            if entry.reserved {
                out.push_str(" reserved");
            }
            out.push('\n');
        }
        out
    }

    /// Write the rendered lockfile atomically in its destination directory.
    ///
    /// # Errors
    ///
    /// Returns an I/O error from creating, writing, syncing, or renaming the temporary file.
    pub fn write_atomic(&self, path: &Path) -> Result<()> {
        let parent = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."));
        let name = path
            .file_name()
            .with_context(|| format!("lockfile path `{}` has no file name", path.display()))?;
        let sequence = TEMP_COUNTER.fetch_add(1, Ordering::Relaxed);
        let temporary = parent.join(format!(
            ".{}.{}.{}.tmp",
            name.to_string_lossy(),
            std::process::id(),
            sequence
        ));
        let result = (|| {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&temporary)
                .with_context(|| format!("failed to create {}", temporary.display()))?;
            file.write_all(self.render().as_bytes())
                .with_context(|| format!("failed to write {}", temporary.display()))?;
            file.sync_all()
                .with_context(|| format!("failed to sync {}", temporary.display()))?;
            fs::rename(&temporary, path)
                .with_context(|| format!("failed to replace {}", path.display()))?;
            Ok(())
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temporary);
        }
        result
    }

    /// Check that an older lockfile's entries all survive unchanged in this one, so CI can
    /// refuse a lockfile edit that deletes, renumbers, changes, or un-reserves anything.
    ///
    /// # Errors
    ///
    /// Returns an error naming the first entry that was removed, renumbered, changed, or
    /// un-reserved.
    pub fn check_append_only(&self, older: &WireLock) -> Result<()> {
        for (key, old) in &older.entries {
            match self.entries.get(key) {
                None => bail!("`{key}` was removed from the lockfile; entries are never deleted"),
                Some(new) if new.number != old.number => bail!(
                    "`{key}` was renumbered from {} to {}; numbers never change",
                    old.number,
                    new.number
                ),
                Some(new) if new.kind != old.kind || new.ty != old.ty => {
                    bail!("`{key}` changed kind or type; entries are append-only")
                }
                Some(new) if old.reserved && !new.reserved => {
                    bail!("`{key}` was un-reserved; a reserved number is never reused");
                }
                Some(_) => {}
            }
        }
        Ok(())
    }

    fn next_number(&self, parent: &str) -> Result<u32> {
        let max = self
            .entries
            .iter()
            .filter(|(key, _)| key.parent() == parent)
            .map(|(_, entry)| entry.number)
            .max()
            .unwrap_or(0);
        let mut next = max
            .checked_add(1)
            .with_context(|| format!("no protobuf field numbers left in `{parent}`"))?;
        if PROTOBUF_RESERVED.contains(&next) {
            next = PROTOBUF_RESERVED.end() + 1;
        }
        if next > PROTOBUF_MAX {
            bail!("no protobuf field numbers left in `{parent}`");
        }
        Ok(next)
    }

    fn validate(&self) -> Result<()> {
        let mut seen: BTreeMap<(String, u32), &MemberKey> = BTreeMap::new();
        for (key, entry) in &self.entries {
            if let Some(other) = seen.insert((key.parent(), entry.number), key) {
                bail!("`{other}` and `{key}` share number {}", entry.number);
            }
        }
        Ok(())
    }
}

fn parse_entry(rest: &str, line_no: usize) -> Result<(u32, MemberKind, Option<String>, bool)> {
    let mut words = rest.split_whitespace();
    let number_token = words
        .next()
        .with_context(|| format!("line {line_no}: missing number"))?;
    if !number_token.chars().all(|c| c.is_ascii_digit()) {
        bail!("line {line_no}: invalid number `{number_token}`");
    }
    if number_token.len() > 1 && number_token.starts_with('0') {
        bail!("line {line_no}: number `{number_token}` has a leading zero");
    }
    let number: u32 = number_token
        .parse()
        .with_context(|| format!("line {line_no}: invalid number `{number_token}`"))?;
    let kind_token = words
        .next()
        .with_context(|| format!("line {line_no}: missing member kind"))?;
    let kind: MemberKind = kind_token
        .parse()
        .map_err(|_| anyhow::anyhow!("line {line_no}: unknown member kind `{kind_token}`"))?;

    let (ty, reserved) = match words.next() {
        None => (None, false),
        Some("reserved") => (None, true),
        Some(ty) => {
            validate_canonical_type(ty)
                .with_context(|| format!("line {line_no}: invalid canonical type"))?;
            let reserved = match words.next() {
                None => false,
                Some("reserved") => true,
                Some(other) => bail!("line {line_no}: unexpected `{other}`"),
            };
            (Some(ty.to_string()), reserved)
        }
    };
    if words.next().is_some() {
        bail!("line {line_no}: trailing text");
    }

    let requires_type = matches!(kind, MemberKind::Field | MemberKind::Param);
    let forbids_type = matches!(kind, MemberKind::EnumCase | MemberKind::Flag);
    if requires_type && ty.is_none() {
        bail!("line {line_no}: `{kind}` requires a canonical type");
    }
    if forbids_type && ty.is_some() {
        bail!("line {line_no}: `{kind}` forbids a canonical type");
    }
    Ok((number, kind, ty, reserved))
}

fn parse_package(value: &str) -> Result<PackageName> {
    let (namespace, name) = value
        .split_once(':')
        .with_context(|| format!("malformed package `{value}`: missing `:`"))?;
    ensure_ident("package namespace", namespace)?;
    ensure_ident("package name", name)?;
    Ok(PackageName {
        namespace: namespace.to_string(),
        name: name.to_string(),
    })
}

fn ensure_ident(what: &str, value: &str) -> Result<()> {
    if !valid_ident(value) {
        bail!("malformed {what} `{value}`: expected a WIT identifier");
    }
    Ok(())
}

fn ensure_no_separator(what: &str, value: &str, separator: char) -> Result<()> {
    if value.contains(separator) {
        bail!("malformed {what} `{value}`: contains `{separator}`");
    }
    Ok(())
}

fn valid_ident(value: &str) -> bool {
    if value.is_empty() {
        return false;
    }
    for (index, part) in value.split('-').enumerate() {
        let Some(first) = part.chars().next() else {
            return false;
        };
        if index == 0 && !first.is_ascii_alphabetic() {
            return false;
        }
        let mut upper = None;
        for ch in part.chars() {
            if ch.is_ascii_digit() {
                continue;
            }
            if ch.is_ascii_uppercase() {
                if upper == Some(false) {
                    return false;
                }
                upper = Some(true);
            } else if ch.is_ascii_lowercase() {
                if upper == Some(true) {
                    return false;
                }
                upper = Some(false);
            } else {
                return false;
            }
        }
    }
    true
}

fn valid_number(number: u32) -> bool {
    (1..=PROTOBUF_MAX).contains(&number) && !PROTOBUF_RESERVED.contains(&number)
}

/// Validate the lockfile's canonical type grammar.
fn validate_canonical_type(value: &str) -> Result<()> {
    let mut parser = CanonicalTypeParser::new(value);
    parser.parse_type()?;
    if parser.position != value.len() {
        bail!("trailing text `{}`", &value[parser.position..]);
    }
    Ok(())
}

struct CanonicalTypeParser<'a> {
    input: &'a str,
    position: usize,
}

impl<'a> CanonicalTypeParser<'a> {
    fn new(input: &'a str) -> Self {
        Self { input, position: 0 }
    }

    fn parse_type(&mut self) -> Result<()> {
        let ident = self.ident()?;
        if ident == "list" && self.eat('<') {
            self.parse_type()?;
            self.expect('>')?;
            return Ok(());
        }
        if ident == "option" && self.eat('<') {
            self.parse_type()?;
            self.expect('>')?;
            return Ok(());
        }
        if ident == "result" && self.eat('<') {
            self.parse_slot()?;
            self.expect(',')?;
            self.parse_slot()?;
            self.expect('>')?;
            return Ok(());
        }
        if self.peek() == Some('/') {
            self.expect('/')?;
            self.ident()?;
            return Ok(());
        }
        if matches!(
            ident,
            "bool"
                | "u8"
                | "u16"
                | "u32"
                | "u64"
                | "s8"
                | "s16"
                | "s32"
                | "s64"
                | "f32"
                | "f64"
                | "string"
        ) {
            return Ok(());
        }
        bail!("unknown type name `{ident}`")
    }

    fn parse_slot(&mut self) -> Result<()> {
        if self.eat('_') {
            Ok(())
        } else {
            self.parse_type()
        }
    }

    fn ident(&mut self) -> Result<&'a str> {
        let start = self.position;
        let first = self
            .input
            .get(start..)
            .and_then(|rest| rest.chars().next())
            .context("missing type name")?;
        if !first.is_ascii_alphabetic() {
            bail!("invalid type name start `{first}`");
        }
        self.position += first.len_utf8();
        while let Some(c) = self.peek() {
            if c.is_ascii_alphanumeric() || c == '-' {
                self.position += c.len_utf8();
            } else {
                break;
            }
        }
        let ident = &self.input[start..self.position];
        if !valid_ident(ident) {
            bail!("invalid type identifier `{ident}`");
        }
        Ok(ident)
    }

    fn peek(&self) -> Option<char> {
        self.input[self.position..].chars().next()
    }

    fn eat(&mut self, expected: char) -> bool {
        if self.peek() == Some(expected) {
            self.position += expected.len_utf8();
            true
        } else {
            false
        }
    }

    fn expect(&mut self, expected: char) -> Result<()> {
        if self.eat(expected) {
            Ok(())
        } else {
            bail!("expected `{expected}`")
        }
    }
}

//! The wire lockfile: stable protobuf numbers for a contract whose source has none.
//!
//! WIT records, variants, enums, and flags are positional and carry no field numbers, but the
//! protobuf projection needs a number per member that never changes. The lockfile maps every
//! numbered member to a number, unique within its parent, and is append-only.
//!
//! The v2 text format is one entry per line, sorted by key:
//! `key = number kind [type] [reserved]`. Lines after the first header line starting with `#`
//! are comments, and blank lines are ignored. An optional additive order section follows the
//! entries; it records per-parent declaration-order fingerprints without changing the entry
//! format (see `docs/design/lockfile.md` section 16).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::{self, Write as _};
use std::fs::{self, OpenOptions};
use std::io::Write as _;
use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result, bail};

use crate::model::{MAX_CANONICAL_TYPE_BYTES, MAX_TYPE_NESTING, Member, MemberKind};

/// The exact first line of every v2 lockfile.
pub const HEADER: &str = "# witforge wire lockfile v2";

/// The marker that starts the additive declaration-order section.
pub const ORDER_HEADER: &str = "# witforge order v1";

/// Protobuf reserves field numbers 19000 to 19999 for its own implementation.
const PROTOBUF_RESERVED: std::ops::RangeInclusive<u32> = 19_000..=19_999;
/// The largest valid protobuf field number.
const PROTOBUF_MAX: u32 = 536_870_911;

static TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

/// A package name in a lockfile key: `namespace:name`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
pub struct PackageName {
    /// The package namespace.
    pub namespace: String,
    /// The package name.
    pub name: String,
}

impl PackageName {
    /// Create a lockfile package name.
    #[must_use]
    pub fn new(namespace: String, name: String) -> Self {
        Self { namespace, name }
    }
}

impl fmt::Display for PackageName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}:{}", self.namespace, self.name)
    }
}

/// A parsed, typed lockfile key.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[non_exhaustive]
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
            ensure_ident("interface", interface)?;
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

    /// The leaf member, function, or parameter name inside its parent scope.
    #[must_use]
    pub(crate) fn member_name(&self) -> &str {
        match self {
            Self::TypeMember { member, .. } => member,
            Self::Method { function, .. } => function,
            Self::Parameter { parameter, .. } => parameter,
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
#[non_exhaustive]
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

impl Entry {
    /// Create a lockfile entry.
    #[must_use]
    pub fn new(number: u32, kind: MemberKind, ty: Option<String>, reserved: bool) -> Self {
        Self {
            number,
            kind,
            ty,
            reserved,
        }
    }
}

/// One parent's declaration-order fingerprint and the names it fingerprints.
///
/// The fingerprint is a stable FNV-1a 64 hash over each member name in source order, with one
/// NUL byte after each name. The names are kept beside it so a later version can compare the
/// relative order of members common to both versions without treating an addition or removal
/// as a reorder.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Order {
    /// The FNV-1a 64 fingerprint of `members`.
    fingerprint: u64,
    /// Live member names in declaration order.
    members: Vec<String>,
}

impl Order {
    fn new(members: Vec<String>) -> Self {
        let fingerprint = fingerprint(&members);
        Self {
            fingerprint,
            members,
        }
    }
}

/// A parsed lockfile.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct WireLock {
    entries: BTreeMap<MemberKey, Entry>,
    orders: BTreeMap<String, Order>,
    /// Whether the parsed text contained the additive order-section marker.
    ///
    /// A released lockfile written before the section existed has no marker and stays accepted;
    /// an unreleased lockfile without one is stale and `lock` adds it.
    orders_present: bool,
}

/// The classification baseline for a lock operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum LockMode<'a> {
    /// Treat every existing entry as released; the default conservative mode.
    Default,
    /// Treat every existing entry as a draft; for a version with no previous baseline.
    First,
    /// Treat entries from `previous` as released and all others as drafts.
    Against(&'a WireLock),
}

/// One released or structural lockfile discrepancy.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct Discrepancy {
    /// The lockfile key the discrepancy concerns, or the order parent for a reorder.
    pub key: String,
    /// The actionable description.
    pub message: String,
}

/// What a reconciliation changed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
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

/// The changes a reconciliation would make and every discrepancy it refused.
///
/// The lockfile is left unchanged when `discrepancies` is non-empty or `changes.reintroduced`
/// is non-empty. Use this type when a caller must report every refusal instead of the first.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct Reconciliation {
    /// The changes that a successful reconciliation would make.
    pub changes: Changes,
    /// Released or structural discrepancies, in deterministic key order.
    pub discrepancies: Vec<Discrepancy>,
    /// Draft removals that would be restored and need `lock` to un-reserve them.
    pub restored: Vec<(String, u32)>,
}

impl Reconciliation {
    /// Whether the lockfile already matches the contract: nothing changed and nothing was
    /// refused.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.changes.is_empty() && self.discrepancies.is_empty() && self.restored.is_empty()
    }
}

fn push_discrepancy(
    discrepancies: &mut Vec<Discrepancy>,
    key: impl Into<String>,
    message: impl Into<String>,
) {
    let key = key.into();
    let message = message.into();
    if !discrepancies
        .iter()
        .any(|found| found.key == key && found.message == message)
    {
        discrepancies.push(Discrepancy { key, message });
    }
}

impl WireLock {
    /// Parse the v2 lockfile text.
    ///
    /// # Errors
    ///
    /// Returns an error for a missing or unknown header, a malformed key or entry, an invalid
    /// number, a bad canonical type, a duplicate key, or two members of one parent sharing a
    /// number. A v1 lockfile is refused rather than migrated. The additive order section is
    /// optional; malformed or mismatched order fingerprints are refused.
    pub fn parse(text: &str) -> Result<Self> {
        Self::parse_inner(text).map_err(|err| {
            let message = format!("{err:#}");
            let mut diag = crate::diagnostic::Diagnostic::error(
                crate::diagnostic::codes::PARSE_ERROR,
                message,
            );
            if let Some(position) = parse_error_position(text, &diag.message) {
                diag = diag.with_position(position);
            }
            anyhow::Error::new(crate::DiagnosticError::single(diag))
        })
    }

    /// Map each member key in a v2 lockfile to its 1-based UTF-16 line [`crate::diagnostic::Position`].
    ///
    /// Invalid or malformed lines are skipped so callers can still locate surrounding entries.
    #[must_use]
    pub fn entry_positions(text: &str) -> BTreeMap<String, crate::diagnostic::Position> {
        let mut out = BTreeMap::new();
        for (index, raw) in text.lines().enumerate() {
            let line_no = index + 1;
            let line = raw
                .strip_prefix('\u{feff}')
                .unwrap_or(raw)
                .trim_matches(is_field_separator);
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            if let Some((key_text, _)) = line.split_once('=') {
                let key = key_text.trim_matches(is_field_separator);
                if !key.is_empty() {
                    out.entry(key.to_string())
                        .or_insert_with(|| crate::line_position(line_no, raw));
                }
            }
        }
        out
    }

    fn parse_inner(text: &str) -> Result<Self> {
        let mut lines = text.lines();
        let Some(first) = lines.next() else {
            bail!("empty lockfile: expected `{HEADER}`");
        };
        if first != HEADER {
            bail!("line 1: expected `{HEADER}`");
        }

        let mut entries = BTreeMap::new();
        let mut numbers: BTreeMap<(String, u32), (MemberKey, usize)> = BTreeMap::new();
        let mut orders = BTreeMap::new();
        let mut saw_order_header = false;
        for (index, raw) in lines.enumerate() {
            let line_no = index + 2;
            let line = raw.trim_matches(is_field_separator);
            if line.is_empty() {
                continue;
            }
            if line == ORDER_HEADER {
                if saw_order_header {
                    bail!("line {line_no}: duplicate `{ORDER_HEADER}`");
                }
                saw_order_header = true;
                continue;
            }
            // An order line before its marker is ignored like any other comment, so a
            // hand-written comment cannot make an otherwise readable lockfile fail to parse.
            if let Some(rest) = line.strip_prefix("# order ")
                && saw_order_header
            {
                let (parent, order) = parse_order(rest, line_no)?;
                if orders.insert(parent.clone(), order).is_some() {
                    bail!("line {line_no}: duplicate order fingerprint for `{parent}`");
                }
                continue;
            }
            if line.starts_with('#') {
                continue;
            }
            let (key_text, rest) = line
                .split_once('=')
                .with_context(|| format!("line {line_no}: expected `key = number kind [type]`"))?;
            let key = MemberKey::parse(key_text.trim_matches(is_field_separator))
                .with_context(|| format!("line {line_no}"))?;
            let (number, kind, ty, reserved) = parse_entry(rest, line_no)?;
            if !kind_allowed_for(&key, kind) {
                bail!(
                    "line {line_no}: kind `{kind}` is not allowed under key `{key}`; a `#` key \
                     holds `param`, an interface key holds `method`, and a type key holds \
                     `field`, `variant-case`, `enum-case`, or `flag`"
                );
            }
            if !valid_number(number) {
                bail!("line {line_no}: {number} is not a usable protobuf field number");
            }
            if let Some((other, other_line)) =
                numbers.insert((key.parent(), number), (key.clone(), line_no))
            {
                bail!(
                    "line {line_no}: `{other}` (line {other_line}) and `{key}` share number \
                     {number}"
                );
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
        let lock = Self {
            entries,
            orders,
            orders_present: saw_order_header,
        };
        Ok(lock)
    }

    /// Iterate entries in canonical key order.
    pub fn entries(&self) -> impl Iterator<Item = (&MemberKey, &Entry)> {
        self.entries.iter()
    }

    /// Iterate declaration-order entries in sorted parent order as `(parent, fingerprint, members)`.
    pub(crate) fn order_fingerprints(&self) -> impl Iterator<Item = (&str, u64, &[String])> {
        self.orders
            .iter()
            .map(|(parent, order)| (parent.as_str(), order.fingerprint, order.members.as_slice()))
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
    /// This returns the first released discrepancy, order change, or mixed-kind parent as an
    /// error and leaves the lockfile unchanged. Use [`WireLock::reconcile_report`] to collect
    /// every discrepancy in one run.
    ///
    /// # Errors
    ///
    /// Returns an error when a member key is malformed, a released discrepancy or order change
    /// is found, a draft removal needs restoring, a parent mixes kinds, or a parent has used
    /// every usable protobuf field number.
    pub fn reconcile(&mut self, members: &[Member]) -> Result<Changes> {
        self.reconcile_with(members, LockMode::Default)
    }

    /// Bring the lockfile up to date with `members` under an explicit classification mode.
    ///
    /// The operation is transactional: a malformed key, exhausted number space, released
    /// discrepancy, or reintroduced member leaves this lockfile unchanged. Use
    /// [`WireLock::reconcile_report_with`] to collect every discrepancy instead of the first.
    ///
    /// # Errors
    ///
    /// Returns an error when the baseline is inconsistent, a member key is malformed, a
    /// released discrepancy or order change is found, a draft removal needs restoring, a
    /// parent mixes kinds, or a parent has used every usable protobuf field number.
    pub fn reconcile_with(&mut self, members: &[Member], mode: LockMode<'_>) -> Result<Changes> {
        let report = self.reconcile_report_with(members, mode)?;
        if let Some(discrepancy) = report.discrepancies.first() {
            bail!("{}", discrepancy.message);
        }
        if let Some((key, _)) = report.restored.first() {
            bail!("draft removal of `{key}` can be restored by locking against the baseline");
        }
        Ok(report.changes)
    }

    /// Reconcile like [`WireLock::reconcile`] and report every discrepancy.
    ///
    /// # Errors
    ///
    /// Returns an error when a member key is malformed or a parent has used every usable
    /// protobuf field number.
    pub fn reconcile_report(&mut self, members: &[Member]) -> Result<Reconciliation> {
        self.reconcile_report_with(members, LockMode::Default)
    }

    /// Reconcile like [`WireLock::reconcile_with`] and report every discrepancy.
    ///
    /// # Errors
    ///
    /// Returns an error when the baseline is inconsistent, a member key is malformed, or a
    /// parent has used every usable protobuf field number.
    pub fn reconcile_report_with(
        &mut self,
        members: &[Member],
        mode: LockMode<'_>,
    ) -> Result<Reconciliation> {
        let mut staged = self.clone();
        let report = staged.reconcile_in_place(members, mode)?;
        if report.discrepancies.is_empty() && report.changes.reintroduced.is_empty() {
            *self = staged;
        }
        Ok(report)
    }

    fn validate_against(
        &self,
        previous: &WireLock,
        live: &BTreeMap<MemberKey, &Member>,
    ) -> Result<Vec<Discrepancy>> {
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
        let mut discrepancies = Vec::new();
        for (key, old) in &previous.entries {
            let Some(current) = self.entries.get(key) else {
                discrepancies.push(Discrepancy {
                    key: key.key(),
                    message: format!("released entry `{key}` is missing from the current lockfile"),
                });
                continue;
            };
            if current.number != old.number {
                discrepancies.push(Discrepancy {
                    key: key.key(),
                    message: format!(
                        "released entry `{key}` was renumbered from {} to {}; numbers never \
                         change",
                        old.number, current.number
                    ),
                });
            }
            if current.kind != old.kind || current.ty != old.ty {
                discrepancies.push(Discrepancy {
                    key: key.key(),
                    message: format!(
                        "released entry `{key}` changed kind or type from `{} {}` to `{} {}`",
                        old.kind,
                        old.ty.as_deref().unwrap_or("-"),
                        current.kind,
                        current.ty.as_deref().unwrap_or("-")
                    ),
                });
            }
            if old.reserved && !current.reserved {
                discrepancies.push(Discrepancy {
                    key: key.key(),
                    message: format!(
                        "released entry `{key}` was un-reserved; reserved numbers are never reused"
                    ),
                });
            }
        }
        Ok(discrepancies)
    }

    #[allow(clippy::too_many_lines)]
    fn reconcile_in_place(
        &mut self,
        members: &[Member],
        mode: LockMode<'_>,
    ) -> Result<Reconciliation> {
        let live: BTreeMap<MemberKey, &Member> = members
            .iter()
            .map(|member| Ok((MemberKey::from_member(member)?, member)))
            .collect::<Result<_>>()?;
        let released: BTreeMap<MemberKey, Entry> = match mode {
            LockMode::Default => self.entries.clone(),
            LockMode::First => BTreeMap::new(),
            LockMode::Against(previous) => previous.entries.clone(),
        };

        let current_orders = orders_from_members(members);
        let empty_orders = BTreeMap::new();
        let baseline_orders = match mode {
            LockMode::Default => &self.orders,
            LockMode::First => &empty_orders,
            LockMode::Against(previous) => &previous.orders,
        };
        let mut changes = Changes::default();
        let mut discrepancies = Vec::new();
        let mut restored = Vec::new();
        for (parent, message) in common_order_discrepancies(&current_orders, baseline_orders) {
            push_discrepancy(&mut discrepancies, parent, message);
        }

        // Every key belongs to the contract's package in every mode; `--against` also checks
        // the previous lockfile's keys.
        if let Some(expected) = live.keys().next().map(MemberKey::package)
            && let Some(key) = self.entries.keys().find(|key| key.package() != expected)
        {
            bail!(
                "lockfile entry `{key}` belongs to package `{}`, not the contract's package \
                 `{expected}`",
                key.package()
            );
        }
        if let LockMode::Against(previous) = mode {
            for discrepancy in self.validate_against(previous, &live)? {
                push_discrepancy(&mut discrepancies, discrepancy.key, discrepancy.message);
            }
        }

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

        // The next number and folded member names for each parent are tracked once. Re-scanning
        // every entry for every new member made a wide contract quadratic.
        let mut next_numbers = initial_next_numbers(&self.entries);
        let mut case_index = build_case_index(&self.entries);

        for member in members {
            let key = MemberKey::from_member(member)?;
            match self.entries.get_mut(&key) {
                Some(entry) if entry.reserved => {
                    let Some(previous) = released.get(&key) else {
                        changes.reintroduced.push((key.key(), entry.number));
                        continue;
                    };
                    if previous.reserved {
                        changes.reintroduced.push((key.key(), entry.number));
                        continue;
                    }
                    // A released entry that the draft reserved only because its member was
                    // removed may be restored. The baseline must still describe the entry,
                    // and the source must describe the same kind and type.
                    if entry.kind != previous.kind || entry.ty != previous.ty {
                        // `validate_against` reports a changed draft entry against the baseline.
                        continue;
                    }
                    if previous.kind != member.kind
                        || previous.ty.as_deref() != member.ty.as_deref()
                    {
                        push_discrepancy(
                            &mut discrepancies,
                            key.key(),
                            format!(
                                "released entry `{key}` changed kind or type from `{} {}` to `{} {}`",
                                previous.kind,
                                previous.ty.as_deref().unwrap_or("-"),
                                member.kind,
                                member.ty.as_deref().unwrap_or("-")
                            ),
                        );
                        continue;
                    }
                    entry.reserved = false;
                    restored.push((key.key(), entry.number));
                    continue;
                }
                Some(entry)
                    if entry.kind != member.kind || entry.ty.as_deref() != member.ty.as_deref() =>
                {
                    if released.contains_key(&key) {
                        push_discrepancy(
                            &mut discrepancies,
                            key.key(),
                            format!(
                                "released entry `{key}` changed kind or type from `{} {}` to `{} {}`",
                                entry.kind,
                                entry.ty.as_deref().unwrap_or("-"),
                                member.kind,
                                member.ty.as_deref().unwrap_or("-")
                            ),
                        );
                        continue;
                    }
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
            let folded = (key.parent(), key.member_name().to_ascii_lowercase());
            if let Some(existing) = case_index.get(&folded)
                && existing.name != key.member_name()
            {
                push_discrepancy(
                    &mut discrepancies,
                    key.key(),
                    case_collision_message(&key, existing),
                );
                continue;
            }
            let number = next_number_in(&mut next_numbers, &key.parent())?;
            self.entries.insert(
                key.clone(),
                Entry {
                    number,
                    kind: member.kind,
                    ty: member.ty.clone(),
                    reserved: false,
                },
            );
            case_index.insert(
                folded,
                CaseAnchor {
                    name: key.member_name().to_string(),
                    reserved: false,
                    number,
                },
            );
            changes.added.push((key.key(), number));
        }
        // One parent has one kind. A type that changes kind keeps its old entries reserved, and
        // adding the new kind's members would leave both kinds under the same parent; the
        // schema of every field that refers to the type has already changed. Refuse that here,
        // in the shared reconcile/drift path, so `check`, `lock`, and every generator report
        // the same violation. A lockfile that already mixes kinds is refused by the same check.
        for (parent, message) in self.kind_discrepancies() {
            push_discrepancy(&mut discrepancies, parent, message);
        }
        // Two entries in one parent that differ only in ASCII letter case share one generated
        // identifier namespace (including reserved entries), so refuse pre-existing or live
        // collisions here as well.
        for (key, message) in self.case_collision_discrepancies() {
            push_discrepancy(&mut discrepancies, key, message);
        }
        // A reintroduced member is a contract violation; leave the lockfile (including its
        // order section) untouched so the caller cannot mistake it for a usable update.
        if changes.reintroduced.is_empty() && discrepancies.is_empty() {
            self.orders_present = !current_orders.is_empty();
            self.orders = current_orders;
        }
        discrepancies.sort_by(|left, right| left.key.cmp(&right.key));
        restored.sort_by(|left, right| left.0.cmp(&right.0));
        Ok(Reconciliation {
            changes,
            discrepancies,
            restored,
        })
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
        let report = self.drift_report_with(members, mode)?;
        if let Some(discrepancy) = report.discrepancies.first() {
            bail!("{}", discrepancy.message);
        }
        if let Some((key, _)) = report.restored.first() {
            bail!("draft removal of `{key}` can be restored by locking against the baseline");
        }
        Ok(report.changes)
    }

    /// Like [`WireLock::drift`] and report every discrepancy.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`WireLock::reconcile_report`].
    pub fn drift_report(&self, members: &[Member]) -> Result<Reconciliation> {
        self.drift_report_with(members, LockMode::Default)
    }

    /// Like [`WireLock::drift_with`] and report every discrepancy.
    ///
    /// # Errors
    ///
    /// Returns the same errors as [`WireLock::reconcile_report_with`].
    pub fn drift_report_with(
        &self,
        members: &[Member],
        mode: LockMode<'_>,
    ) -> Result<Reconciliation> {
        let mut copy = self.clone();
        copy.reconcile_report_with(members, mode)
    }

    /// Report live or existing lockfile members in one parent that differ only in ASCII letter
    /// case, with reserved entries included.
    ///
    /// Draft entries that `mode` drops do not collide. Each collision is returned as a `WF1003`
    /// diagnostic matching the discrepancy reported by [`WireLock::reconcile_report_with`] and
    /// [`WireLock::drift_report_with`].
    ///
    /// # Errors
    ///
    /// Returns an error when a member key is malformed.
    pub fn case_collisions(
        &self,
        members: &[Member],
        mode: LockMode<'_>,
    ) -> Result<Vec<crate::diagnostic::Diagnostic>> {
        let live: BTreeMap<MemberKey, &Member> = members
            .iter()
            .map(|member| Ok((MemberKey::from_member(member)?, member)))
            .collect::<Result<_>>()?;
        let released: BTreeMap<MemberKey, Entry> = match mode {
            LockMode::Default => self.entries.clone(),
            LockMode::First => BTreeMap::new(),
            LockMode::Against(previous) => previous.entries.clone(),
        };
        let mut staged = self.entries.clone();
        staged.retain(|key, entry| {
            released.contains_key(key) || (!entry.reserved && live.contains_key(key))
        });
        for (key, entry) in &mut staged {
            if released.contains_key(key) && !entry.reserved && !live.contains_key(key) {
                entry.reserved = true;
            }
        }
        let mut next_numbers = initial_next_numbers(&staged);
        let mut case_index = build_case_index(&staged);
        let mut collisions = Vec::new();
        for member in members {
            let key = MemberKey::from_member(member)?;
            if staged.contains_key(&key) {
                continue;
            }
            let folded = (key.parent(), key.member_name().to_ascii_lowercase());
            if let Some(existing) = case_index.get(&folded)
                && existing.name != key.member_name()
            {
                let message = case_collision_message(&key, existing);
                let path = key.key();
                if !collisions
                    .iter()
                    .any(|(found_path, found_msg)| found_path == &path && found_msg == &message)
                {
                    collisions.push((path, message));
                }
                continue;
            }
            let number = next_number_in(&mut next_numbers, &key.parent())?;
            case_index.insert(
                folded,
                CaseAnchor {
                    name: key.member_name().to_string(),
                    reserved: false,
                    number,
                },
            );
        }
        for (path, message) in entries_case_collisions(&staged) {
            if !collisions
                .iter()
                .any(|(found_path, found_msg)| found_path == &path && found_msg == &message)
            {
                collisions.push((path, message));
            }
        }
        collisions.sort_by(|left, right| left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1)));
        Ok(collisions
            .into_iter()
            .map(|(path, message)| {
                crate::diagnostic::Diagnostic::error("WF1003", message).with_path(path)
            })
            .collect())
    }

    /// Check that every entry and order fingerprint belongs to `package`, the contract's
    /// package.
    ///
    /// Reconciling checks entries against the package of the contract's members, so a contract
    /// with no members needs this check to refuse a lockfile written for another package.
    ///
    /// # Errors
    ///
    /// Returns an error naming the first entry or order parent of another package.
    pub fn check_package(&self, package: &PackageName) -> Result<()> {
        if let Some(key) = self.entries.keys().find(|key| key.package() != package) {
            bail!(
                "lockfile entry `{key}` belongs to package `{}`, not the contract's package \
                 `{package}`",
                key.package()
            );
        }
        let prefix = format!("{package}/");
        if let Some(parent) = self
            .orders
            .keys()
            .find(|parent| !parent.starts_with(&prefix))
        {
            bail!(
                "order fingerprint `{parent}` belongs to another package, not the contract's \
                 package `{package}`"
            );
        }
        Ok(())
    }

    /// Validate the additive order section against the contract's live members.
    ///
    /// The section must contain exactly one line for every parent with ordered members, naming
    /// exactly that parent's live members in declaration order. A line for another parent, a
    /// missing line, or a line naming a subset, a non-member, or the wrong order is an error.
    ///
    /// `allow_missing_section` is for a released lockfile written before the section existed: a
    /// missing marker is accepted, but a marker with a wrong or incomplete section is not.
    ///
    /// # Errors
    ///
    /// Returns an error naming the first parent whose line is missing, extra, or different.
    pub fn validate_order_section(
        &self,
        members: &[Member],
        allow_missing_section: bool,
    ) -> Result<()> {
        let expected = orders_from_members(members);
        if !self.orders_present {
            if allow_missing_section || expected.is_empty() {
                return Ok(());
            }
            bail!(
                "lockfile has no `{ORDER_HEADER}` section; the contract has ordered members and \
                 `witforge lock` writes them"
            );
        }
        if let Some(parent) = self
            .orders
            .keys()
            .find(|parent| !expected.contains_key(*parent))
        {
            bail!("lockfile has an order line for `{parent}`, which has no live members");
        }
        for (parent, expected_order) in &expected {
            let Some(found) = self.orders.get(parent) else {
                bail!("lockfile is missing the order line for `{parent}`");
            };
            if found.members != expected_order.members {
                bail!(
                    "order line for `{parent}` names `{}`, expected `{}`",
                    found.members.join(" "),
                    expected_order.members.join(" ")
                );
            }
        }
        Ok(())
    }

    /// Refuse structurally wrong order lines without requiring the section to be current.
    ///
    /// `diff` deliberately accepts a stale order so it can report a reorder, but a line for an
    /// unknown parent, or one naming a non-member, is not stale: it is inconsistent with the
    /// contract and cannot describe any earlier order.
    ///
    /// # Errors
    ///
    /// Returns an error naming the first order line that does not describe its parent's live
    /// members.
    pub(crate) fn validate_order_structure(&self, members: &[Member]) -> Result<()> {
        let expected = orders_from_members(members);
        for (parent, order) in &self.orders {
            let Some(expected_order) = expected.get(parent) else {
                bail!("lockfile has an order line for `{parent}`, which has no live members");
            };
            let found: BTreeSet<&str> = order.members.iter().map(String::as_str).collect();
            let expected_names: BTreeSet<&str> =
                expected_order.members.iter().map(String::as_str).collect();
            if found != expected_names {
                bail!(
                    "order line for `{parent}` names `{}`, expected the live members `{}`",
                    order.members.join(" "),
                    expected_order.members.join(" ")
                );
            }
        }
        Ok(())
    }

    /// Whether `text` is exactly this lockfile's [rendering](WireLock::render): the form
    /// `witforge lock` writes, with no comments, blank lines, extra whitespace, CR, or other
    /// order.
    #[must_use]
    pub fn is_canonical(&self, text: &str) -> bool {
        self.render() == text
    }

    /// Render the lockfile text (sorted, stable, LF line endings).
    ///
    /// Entries are sorted by key. When order fingerprints are present, the additive order
    /// section follows them, sorted by parent key.
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
        if !self.orders.is_empty() {
            out.push_str(ORDER_HEADER);
            out.push('\n');
            for (parent, order) in &self.orders {
                let _ = writeln!(
                    out,
                    "# order {parent} {:016x} = {}",
                    order.fingerprint,
                    order.members.join(" ")
                );
            }
        }
        out
    }

    /// Write the rendered lockfile atomically in its destination directory.
    ///
    /// A file that already holds exactly the rendered text is left alone, so an unchanged
    /// lockfile keeps its inode, timestamps, and permissions.
    ///
    /// # Errors
    ///
    /// Returns an I/O error from creating, writing, syncing, or renaming the temporary file.
    pub fn write_atomic(&self, path: &Path) -> Result<()> {
        let rendered = self.render();
        Self::write_rendered_atomic(path, &rendered)
    }

    pub(crate) fn write_rendered_atomic(path: &Path, rendered: &str) -> Result<()> {
        crate::ensure_lockfile_not_symlink(path)?;
        if fs::read(path).is_ok_and(|existing| existing == rendered.as_bytes()) {
            return Ok(());
        }
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
            file.write_all(rendered.as_bytes())
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
    /// refuse a lockfile edit that deletes, renumbers, changes, or un-reserves anything. When
    /// both files have order fingerprints, it also refuses a reorder of common members.
    ///
    /// # Errors
    ///
    /// Returns an error naming the first entry that was removed, renumbered, changed, or
    /// un-reserved, or the first parent whose common members were reordered.
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
        compare_common_orders(&self.orders, &older.orders)
    }

    /// Refuse a parent whose live and reserved entries do not all have one kind.
    ///
    /// A named type has one kind, so its lockfile parent holds `field`, `variant-case`,
    /// `enum-case`, or `flag` entries, never a mix. When a released type changes kind its old
    /// entries stay reserved, and reconciliation would add the new kind beside them; the wire
    /// shape of every referring member has already changed even when the old numbers survive.
    /// Interface parents hold `method` entries and function parents `param` entries by
    /// construction, so this also rejects a hand-written lockfile that mixes those.
    ///
    /// This is deliberately not part of [`WireLock::parse`]: `diff` must be able to load a
    /// mixed-kind endpoint and report the kind change. `check`, `lock`, and the generators
    /// reach it through [`WireLock::drift`] or [`WireLock::reconcile`], which makes the
    /// refusal a contract violation (exit `1`) rather than a parse error (exit `2`).
    fn kind_discrepancies(&self) -> Vec<(String, String)> {
        let mut kinds: BTreeMap<String, MemberKind> = BTreeMap::new();
        let mut reported: BTreeSet<String> = BTreeSet::new();
        let mut discrepancies = Vec::new();
        for (key, entry) in &self.entries {
            let parent = key.parent();
            match kinds.get(&parent) {
                Some(&kind) if kind != entry.kind => {
                    if reported.insert(parent.clone()) {
                        discrepancies.push((
                            parent.clone(),
                            format!(
                                "lockfile parent `{parent}` mixes member kinds `{kind}` and \
                                 `{}`; a type, interface, or function keeps one kind, so give \
                                 the changed definition a new name in a new version",
                                entry.kind
                            ),
                        ));
                    }
                }
                Some(_) => {}
                None => {
                    kinds.insert(parent, entry.kind);
                }
            }
        }
        discrepancies
    }

    fn case_collision_discrepancies(&self) -> Vec<(String, String)> {
        entries_case_collisions(&self.entries)
    }
}

struct CaseAnchor {
    name: String,
    reserved: bool,
    number: u32,
}

fn build_case_index(
    entries: &BTreeMap<MemberKey, Entry>,
) -> BTreeMap<(String, String), CaseAnchor> {
    let mut index: BTreeMap<(String, String), CaseAnchor> = BTreeMap::new();
    for (key, entry) in entries {
        let folded = (key.parent(), key.member_name().to_ascii_lowercase());
        let candidate = CaseAnchor {
            name: key.member_name().to_string(),
            reserved: entry.reserved,
            number: entry.number,
        };
        match index.entry(folded) {
            std::collections::btree_map::Entry::Vacant(slot) => {
                slot.insert(candidate);
            }
            std::collections::btree_map::Entry::Occupied(mut slot) => {
                let current = slot.get();
                if (!candidate.reserved, candidate.number, &candidate.name)
                    < (!current.reserved, current.number, &current.name)
                {
                    slot.insert(candidate);
                }
            }
        }
    }
    index
}

fn case_collision_message(key: &MemberKey, other: &CaseAnchor) -> String {
    let kind = if other.reserved {
        "reserved member"
    } else {
        "member"
    };
    format!(
        "member `{key}` differs only in letter case from {kind} `{}` (number {}) in `{}`; give \
         it a distinct name",
        other.name,
        other.number,
        key.parent()
    )
}

fn entries_case_collisions(entries: &BTreeMap<MemberKey, Entry>) -> Vec<(String, String)> {
    let mut groups: BTreeMap<(String, String), Vec<(&MemberKey, &Entry)>> = BTreeMap::new();
    for (key, entry) in entries {
        groups
            .entry((key.parent(), key.member_name().to_ascii_lowercase()))
            .or_default()
            .push((key, entry));
    }
    let mut discrepancies = Vec::new();
    for mut items in groups.into_values() {
        if items.len() < 2 {
            continue;
        }
        // Anchor on the earliest reserved entry (or lowest-numbered entry) so a live member that
        // collides with a reserved one is the key reported.
        items.sort_by_key(|(key, entry)| (!entry.reserved, entry.number, key.key()));
        let (anchor_key, anchor_entry) = items[0];
        let anchor = CaseAnchor {
            name: anchor_key.member_name().to_string(),
            reserved: anchor_entry.reserved,
            number: anchor_entry.number,
        };
        for &(other_key, _) in &items[1..] {
            discrepancies.push((other_key.key(), case_collision_message(other_key, &anchor)));
        }
    }
    discrepancies
}

fn initial_next_numbers(entries: &BTreeMap<MemberKey, Entry>) -> BTreeMap<String, u32> {
    let mut next_numbers: BTreeMap<String, u32> = BTreeMap::new();
    for (key, entry) in entries {
        let parent = key.parent();
        let candidate = entry.number.checked_add(1).unwrap_or(PROTOBUF_MAX + 1);
        next_numbers
            .entry(parent)
            .and_modify(|next| *next = (*next).max(candidate))
            .or_insert(candidate);
    }
    next_numbers
}

/// Assign the next number in `parent` from the per-parent cursors.
fn next_number_in(next_numbers: &mut BTreeMap<String, u32>, parent: &str) -> Result<u32> {
    let next = next_numbers.entry(parent.to_string()).or_insert(1);
    if PROTOBUF_RESERVED.contains(next) {
        *next = PROTOBUF_RESERVED.end() + 1;
    }
    let number = *next;
    if number > PROTOBUF_MAX {
        bail!("no protobuf field numbers left in `{parent}`");
    }
    *next = number
        .checked_add(1)
        .with_context(|| format!("no protobuf field numbers left in `{parent}`"))?;
    Ok(number)
}

/// Group live members by parent, preserving their source order.
fn orders_from_members(members: &[Member]) -> BTreeMap<String, Order> {
    let mut grouped: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for member in members {
        grouped
            .entry(member.parent.clone())
            .or_default()
            .push(member.name.clone());
    }
    grouped
        .into_iter()
        .map(|(parent, members)| (parent, Order::new(members)))
        .collect()
}

/// Refuse a reorder of members common to a current and a baseline order.
///
/// Members that exist on only one side are ignored, so adding or removing a member does not
/// report a false reorder. If either side has no fingerprint for a parent, there is nothing to
/// compare; that is the migration path for v2 lockfiles written before this section existed.
fn common_order_discrepancies(
    current: &BTreeMap<String, Order>,
    baseline: &BTreeMap<String, Order>,
) -> Vec<(String, String)> {
    let mut discrepancies = Vec::new();
    for (parent, current_order) in current {
        let Some(baseline_order) = baseline.get(parent) else {
            continue;
        };
        let common: BTreeSet<&str> = current_order
            .members
            .iter()
            .map(String::as_str)
            .filter(|name| baseline_order.members.iter().any(|old| old == name))
            .collect();
        let old_common: Vec<&str> = baseline_order
            .members
            .iter()
            .map(String::as_str)
            .filter(|name| common.contains(name))
            .collect();
        let new_common: Vec<&str> = current_order
            .members
            .iter()
            .map(String::as_str)
            .filter(|name| common.contains(name))
            .collect();
        if old_common != new_common {
            discrepancies.push((
                parent.clone(),
                format!(
                    "declaration order of `{parent}` changed for members present in both \
                     versions; append new members instead of reordering"
                ),
            ));
        }
    }
    discrepancies
}

fn compare_common_orders(
    current: &BTreeMap<String, Order>,
    baseline: &BTreeMap<String, Order>,
) -> Result<()> {
    if let Some((_, message)) = common_order_discrepancies(current, baseline).first() {
        bail!("{message}");
    }
    Ok(())
}

/// Parse one `# order <parent> <fingerprint> = <member> ...` line.
fn parse_order(rest: &str, line_no: usize) -> Result<(String, Order)> {
    let (left, names) = rest.split_once(" = ").with_context(|| {
        format!("line {line_no}: expected `<parent> <fingerprint> = <members>`")
    })?;
    let mut left = split_spaces(left);
    let parent = left
        .next()
        .with_context(|| format!("line {line_no}: missing order parent"))?;
    let token = left
        .next()
        .with_context(|| format!("line {line_no}: missing order fingerprint"))?;
    if left.next().is_some() {
        bail!("line {line_no}: unexpected text after the order fingerprint");
    }
    validate_order_parent(parent, line_no)?;
    if token.len() != 16
        || !token
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        bail!(
            "line {line_no}: invalid order fingerprint `{token}`: expected exactly sixteen \
             lowercase hexadecimal digits"
        );
    }
    let fingerprint = u64::from_str_radix(token, 16)
        .with_context(|| format!("line {line_no}: invalid order fingerprint `{token}`"))?;
    let members: Vec<String> = split_spaces(names).map(ToString::to_string).collect();
    if members.is_empty() {
        bail!("line {line_no}: an order fingerprint must name at least one member");
    }
    let mut seen = BTreeSet::new();
    for member in &members {
        ensure_ident("order member", member).with_context(|| format!("line {line_no}"))?;
        if !seen.insert(member.as_str()) {
            bail!("line {line_no}: duplicate member `{member}` in order fingerprint");
        }
    }
    let order = Order::new(members);
    if order.fingerprint != fingerprint {
        bail!("line {line_no}: order fingerprint does not match its member list");
    }
    Ok((parent.to_string(), order))
}

fn validate_order_parent(parent: &str, line_no: usize) -> Result<()> {
    let sample = format!("{parent}.member");
    let parsed = MemberKey::parse(&sample).with_context(|| {
        format!(
            "line {line_no}: invalid order parent `{parent}`: expected a type, interface, or \
             function parent"
        )
    })?;
    if parsed.parent() != parent {
        bail!(
            "line {line_no}: invalid order parent `{parent}`: expected a type, interface, or \
             function parent"
        );
    }
    Ok(())
}

fn is_field_separator(ch: char) -> bool {
    ch == ' ' || ch == '\t'
}

fn split_fields(value: &str) -> impl Iterator<Item = &str> {
    value
        .split(is_field_separator)
        .filter(|field| !field.is_empty())
}

fn split_spaces(value: &str) -> impl Iterator<Item = &str> {
    value.split(' ').filter(|field| !field.is_empty())
}

fn kind_allowed_for(key: &MemberKey, kind: MemberKind) -> bool {
    match key {
        MemberKey::TypeMember { .. } => matches!(
            kind,
            MemberKind::Field | MemberKind::VariantCase | MemberKind::EnumCase | MemberKind::Flag
        ),
        MemberKey::Method { .. } => kind == MemberKind::Method,
        MemberKey::Parameter { .. } => kind == MemberKind::Param,
    }
}

/// Stable FNV-1a 64 over each name followed by a NUL byte.
fn fingerprint(members: &[String]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET;
    for member in members {
        for byte in member.bytes().chain(std::iter::once(0)) {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(PRIME);
        }
    }
    hash
}

fn parse_entry(rest: &str, line_no: usize) -> Result<(u32, MemberKind, Option<String>, bool)> {
    let mut words = split_fields(rest);
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
    if value.len() > MAX_CANONICAL_TYPE_BYTES {
        bail!(
            "canonical type is {} bytes, over the {MAX_CANONICAL_TYPE_BYTES}-byte limit",
            value.len()
        );
    }
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
    depth: usize,
}

impl<'a> CanonicalTypeParser<'a> {
    fn new(input: &'a str) -> Self {
        Self {
            input,
            position: 0,
            depth: 0,
        }
    }

    fn parse_type(&mut self) -> Result<()> {
        let ident = self.ident()?;
        if ident == "list" && self.eat('<') {
            return self.enter_container(|parser| {
                parser.parse_type()?;
                parser.expect('>')
            });
        }
        if ident == "option" && self.eat('<') {
            return self.enter_container(|parser| {
                parser.parse_type()?;
                parser.expect('>')
            });
        }
        if ident == "result" && self.eat('<') {
            return self.enter_container(|parser| {
                parser.parse_slot()?;
                parser.expect(',')?;
                parser.parse_slot()?;
                parser.expect('>')
            });
        }
        if self.eat(':') {
            self.ident()?;
            self.expect('/')?;
            self.ident()?;
            self.expect('/')?;
            self.ident()?;
            return Ok(());
        }
        if self.eat('/') {
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

    fn enter_container(&mut self, parse: impl FnOnce(&mut Self) -> Result<()>) -> Result<()> {
        let next = self.depth + 1;
        if next > MAX_TYPE_NESTING {
            bail!("type nesting exceeds {MAX_TYPE_NESTING} levels");
        }
        self.depth = next;
        let result = parse(self);
        self.depth -= 1;
        result
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

/// Extract a 1-based line position from a [`WireLock::parse`] error message.
fn parse_error_position(text: &str, message: &str) -> Option<crate::diagnostic::Position> {
    if message.starts_with("empty lockfile") {
        return Some(crate::diagnostic::Position {
            line: 1,
            column: 1,
            end_line: 1,
            end_column: 1,
        });
    }
    let rest = message.strip_prefix("line ")?;
    let digits_end = rest
        .bytes()
        .position(|byte| !byte.is_ascii_digit())
        .unwrap_or(rest.len());
    let line_no: usize = rest[..digits_end].parse().ok()?;
    if line_no == 0 {
        return None;
    }
    let raw_line = text.lines().nth(line_no - 1).unwrap_or("");
    Some(crate::line_position(line_no, raw_line))
}

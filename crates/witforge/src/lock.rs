//! The wire lockfile: stable protobuf numbers for a contract whose source has none.
//!
//! WIT records, variants, enums, and flags are positional and carry no field numbers, but the
//! protobuf projection needs a number per member that never changes. The lockfile maps every
//! member key (`namespace:package/interface/type.member`) to a number, unique within its parent
//! type, and is append-only:
//!
//! - a new member gets the next free number in its parent, skipping protobuf's reserved range;
//! - a removed member stays in the file, marked `reserved`, so its number is never reused;
//! - nothing is ever renumbered or deleted.
//!
//! The text format is one entry per line, sorted by key: `key = N` or `key = N reserved`.
//! Lines starting with `#` are comments and blank lines are ignored.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use anyhow::{bail, Context, Result};

use crate::model::Member;

/// Protobuf reserves field numbers 19000 to 19999 for its own implementation.
const PROTOBUF_RESERVED: std::ops::RangeInclusive<u32> = 19_000..=19_999;
/// The largest valid protobuf field number.
const PROTOBUF_MAX: u32 = 536_870_911;

/// One lockfile entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    /// The wire number, unique within the member's parent type.
    pub number: u32,
    /// Whether the member was removed (its number stays taken).
    pub reserved: bool,
}

/// A parsed lockfile.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WireLock {
    entries: BTreeMap<String, Entry>,
}

/// What [`WireLock::reconcile`] changed.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Changes {
    /// Members that were assigned a new number.
    pub added: Vec<(String, u32)>,
    /// Members that disappeared from the contract and are now reserved.
    pub reserved: Vec<(String, u32)>,
}

impl Changes {
    /// Whether the lockfile is already up to date.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.added.is_empty() && self.reserved.is_empty()
    }
}

impl WireLock {
    /// Parse the lockfile text.
    ///
    /// # Errors
    ///
    /// Returns an error for a malformed line, a duplicate key, an invalid number, or two members
    /// of one parent sharing a number.
    pub fn parse(text: &str) -> Result<Self> {
        let mut entries = BTreeMap::new();
        for (index, raw) in text.lines().enumerate() {
            let line_no = index + 1;
            let line = raw.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let (key, rest) = line
                .split_once('=')
                .with_context(|| format!("line {line_no}: expected `key = number`"))?;
            let key = key.trim();
            if key.is_empty() || parent_of(key).is_none() {
                bail!("line {line_no}: malformed key `{key}`");
            }
            let mut words = rest.split_whitespace();
            let number: u32 = words
                .next()
                .with_context(|| format!("line {line_no}: missing number"))?
                .parse()
                .with_context(|| format!("line {line_no}: invalid number"))?;
            let reserved = match words.next() {
                None => false,
                Some("reserved") => true,
                Some(other) => bail!("line {line_no}: unexpected `{other}`"),
            };
            if words.next().is_some() {
                bail!("line {line_no}: trailing text");
            }
            if !valid_number(number) {
                bail!("line {line_no}: {number} is not a usable protobuf field number");
            }
            if entries
                .insert(key.to_string(), Entry { number, reserved })
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
        self.entries.get(key).copied()
    }

    /// Bring the lockfile up to date with `members`: assign numbers to new members and reserve
    /// removed ones. Existing numbers never change.
    pub fn reconcile(&mut self, members: &[Member]) -> Changes {
        let live: BTreeSet<String> = members.iter().map(Member::key).collect();
        let mut changes = Changes::default();

        for (key, entry) in &mut self.entries {
            if !entry.reserved && !live.contains(key) {
                entry.reserved = true;
                changes.reserved.push((key.clone(), entry.number));
            }
        }
        for member in members {
            let key = member.key();
            if self.entries.contains_key(&key) {
                // A reserved key that reappears keeps its reservation: the member is new, so it
                // gets a new number under a new name; reusing the old one would change meaning.
                continue;
            }
            let number = self.next_number(&member.parent);
            self.entries.insert(
                key.clone(),
                Entry {
                    number,
                    reserved: false,
                },
            );
            changes.added.push((key, number));
        }
        changes
    }

    /// Members of the contract that the lockfile does not number, and live lockfile entries whose
    /// member no longer exists. Both empty means the lockfile is current.
    #[must_use]
    pub fn drift(&self, members: &[Member]) -> Changes {
        let mut copy = self.clone();
        copy.reconcile(members)
    }

    /// Render the lockfile text (sorted, stable, LF line endings).
    #[must_use]
    pub fn render(&self) -> String {
        let mut out =
            String::from("# witforge wire lockfile: append-only. Do not edit numbers by hand.\n");
        for (key, entry) in &self.entries {
            let _ = write!(out, "{key} = {}", entry.number);
            if entry.reserved {
                out.push_str(" reserved");
            }
            out.push('\n');
        }
        out
    }

    /// Check that an older lockfile's entries all survive unchanged in this one, so CI can
    /// refuse a lockfile edit that deletes or renumbers anything.
    ///
    /// # Errors
    ///
    /// Returns an error naming the first entry that was removed, renumbered, or un-reserved.
    pub fn check_append_only(&self, older: &WireLock) -> Result<()> {
        for (key, old) in &older.entries {
            match self.entries.get(key) {
                None => bail!("`{key}` was removed from the lockfile; entries are never deleted"),
                Some(new) if new.number != old.number => bail!(
                    "`{key}` was renumbered from {} to {}; numbers never change",
                    old.number,
                    new.number
                ),
                Some(new) if old.reserved && !new.reserved => {
                    bail!("`{key}` was un-reserved; a reserved number is never reused");
                }
                Some(_) => {}
            }
        }
        Ok(())
    }

    fn next_number(&self, parent: &str) -> u32 {
        let max = self
            .entries
            .iter()
            .filter(|(key, _)| parent_of(key) == Some(parent))
            .map(|(_, entry)| entry.number)
            .max()
            .unwrap_or(0);
        let mut next = max + 1;
        if PROTOBUF_RESERVED.contains(&next) {
            next = PROTOBUF_RESERVED.end() + 1;
        }
        next
    }

    fn validate(&self) -> Result<()> {
        let mut seen: BTreeMap<(&str, u32), &str> = BTreeMap::new();
        for (key, entry) in &self.entries {
            let parent = parent_of(key).unwrap_or_default();
            if let Some(other) = seen.insert((parent, entry.number), key) {
                bail!("`{other}` and `{key}` share number {}", entry.number);
            }
        }
        Ok(())
    }
}

fn parent_of(key: &str) -> Option<&str> {
    let (parent, member) = key.rsplit_once('.')?;
    (!parent.is_empty() && !member.is_empty() && parent.contains('/')).then_some(parent)
}

fn valid_number(n: u32) -> bool {
    (1..=PROTOBUF_MAX).contains(&n) && !PROTOBUF_RESERVED.contains(&n)
}

//! Emitters for witforge output formats.
//!
//! Emitters consume the owned [`crate::model::Model`], never `wit-parser` types.

/// Return a [`crate::ContractError`] built from a format string: a contract the emitter cannot
/// represent, such as two WIT names that map to one generated name. Internal errors use plain
/// `bail!`, so the CLI exits with `2` for them rather than reporting a contract violation.
macro_rules! contract_bail {
    ($($arg:tt)*) => {
        return Err(anyhow::Error::new($crate::ContractError::new(format!($($arg)*))))
    };
}

pub mod glue;
pub mod output;
pub mod proto;
pub mod rust;
mod versions;

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::versions::{Version, load_versions};
    use super::{glue, proto, rust};
    use crate::is_contract_error;
    use crate::lock::WireLock;
    use crate::model::{QualifiedName, TypeDefKind, TypeExpr};

    fn versions() -> Vec<Version> {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/versions/wit");
        load_versions(&root).unwrap()
    }

    fn assert_internal(error: &anyhow::Error, expected: &str) {
        let message = format!("{error:#}");
        assert!(message.contains(expected), "{message}");
        assert!(!is_contract_error(error), "{message}");
    }

    /// Validated input never makes an emitter fail internally, so the versions are edited after
    /// validation; each plan must pass the emitter's plain error on rather than call it a
    /// contract violation, which the CLI would report with exit `1` instead of `2`.
    #[test]
    fn plans_pass_on_an_internal_emitter_error_as_internal() {
        // A record field naming a type the model does not have.
        let mut dangling = versions();
        for version in &mut dangling {
            for interface in &mut version.model.interfaces {
                for def in &mut interface.types {
                    if let TypeDefKind::Record(fields) = &mut def.kind {
                        fields[0].ty = TypeExpr::Named(QualifiedName {
                            interface: interface.name.clone(),
                            name: "missing".to_string(),
                        });
                    }
                }
            }
        }
        assert_internal(
            &rust::plan_versions(&dangling).unwrap_err(),
            "is not in the model",
        );
        assert_internal(
            &glue::plan_versions(&dangling, "crate::plain", "crate::proto").unwrap_err(),
            "is not in the model",
        );

        // A lockfile without the model's members.
        let mut unlocked = versions();
        for version in &mut unlocked {
            version.lock = WireLock::default();
        }
        assert_internal(
            &proto::plan_versions(&unlocked).unwrap_err(),
            "lockfile has no",
        );

        // A live member whose entry is reserved: glue converts it, and the protobuf schema glue
        // checks the contract against refuses it.
        let mut reserved = versions();
        for version in &mut reserved {
            let text = version.lock.render().replace(
                "user.name = 1 field string\n",
                "user.name = 1 field string reserved\n",
            );
            assert_ne!(text, version.lock.render());
            version.lock = WireLock::parse(&text).unwrap();
        }
        assert_internal(
            &glue::plan_versions(&reserved, "crate::plain", "crate::proto").unwrap_err(),
            "is live in the model",
        );
    }
}

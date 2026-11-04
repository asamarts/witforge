//! Keep the runnable examples buildable through every emitter.

use std::path::{Path, PathBuf};

use witforge::emit::{glue, json_schema, proto, rust};

fn examples() -> Option<PathBuf> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../examples");
    root.is_dir().then_some(root)
}

#[test]
fn every_example_plans_through_every_emitter() {
    // The `.crate` package carries this test but not the repository-level examples. Skip rather
    // than fail when someone runs the packaged test suite outside a checkout.
    let Some(examples) = examples() else {
        return;
    };
    for root in [examples.join("single/wit"), examples.join("evolved/wit")] {
        let rust_files = rust::plan(&root).unwrap();
        assert!(!rust_files.files().is_empty(), "{}", root.display());
        assert!(!proto::plan(&root).unwrap().files().is_empty());
        assert!(!json_schema::plan(&root).unwrap().files().is_empty());
        let glue_files =
            glue::plan(&root, "crate::generated::rust", "crate::generated::proto").unwrap();
        assert!(!glue_files.files().is_empty());
    }
}

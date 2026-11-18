#![no_main]

use std::fs;
use std::path::PathBuf;

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    if text.len() > 64 * 1024 {
        return;
    }

    // `witforge::load` takes a directory. Keep one directory per worker so parallel fuzz
    // processes cannot overwrite each other's input.
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("target")
        .join(format!("fuzz-wit-{}", std::process::id()));
    let version_dir = root.join("v0.1");
    if fs::create_dir_all(&version_dir).is_err() {
        return;
    }
    if fs::write(version_dir.join("contract.wit"), text).is_err() {
        return;
    }

    let Ok(contract) = witforge::load(&version_dir) else {
        return;
    };
    if !witforge::subset::check(&contract).is_empty() {
        return;
    }
    let Ok(model) = witforge::model::build(&contract) else {
        return;
    };
    let Ok(members) = model.members() else {
        return;
    };
    let mut lock = witforge::lock::WireLock::default();
    if lock.reconcile(&members).is_err() {
        return;
    }

    let _ = witforge::emit::rust::emit_version(&model);
    let _ = witforge::emit::proto::emit_major(&model, &lock);
    let _ = witforge::emit::json_schema::emit_version(&model);
    let _ = witforge::emit::glue::emit_version(&model, &lock, "crate::plain", "crate::proto");
});

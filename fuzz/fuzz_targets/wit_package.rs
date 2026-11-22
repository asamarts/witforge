#![no_main]

#[path = "structured_gen.rs"]
mod structured_gen;

use std::fs;

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    if text.len() > 64 * 1024 {
        return;
    }

    let scratch = structured_gen::ScratchDir::new("wit");
    let version_dir = scratch.path().join("v0.1");
    if fs::create_dir_all(&version_dir).is_err() {
        return;
    }
    if fs::write(version_dir.join("contract.wit"), text).is_err() {
        return;
    }

    let _ = structured_gen::assert_invariants_for_version_dir(scratch.path(), &version_dir);
});

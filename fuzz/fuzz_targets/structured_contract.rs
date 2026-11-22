#![no_main]

#[path = "structured_gen.rs"]
mod structured_gen;

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if data.is_empty() || data.len() > 4096 {
        return;
    }

    let generated = structured_gen::generate_from_bytes(data);
    let scratch = structured_gen::ScratchDir::new("structured");
    let version_dir = scratch.path().join("v0.1");
    generated.write_to(&version_dir);

    let _ = structured_gen::assert_invariants_for_version_dir(scratch.path(), &version_dir);
});

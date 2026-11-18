#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else {
        return;
    };
    if text.len() > 64 * 1024 {
        return;
    }
    if let Ok(lock) = witforge::lock::WireLock::parse(text) {
        let _ = lock.render();
    }
});

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
        let rendered = lock.render();
        let reparsed =
            witforge::lock::WireLock::parse(&rendered).expect("a rendered lockfile must parse");
        assert_eq!(
            lock.entries()
                .map(|(key, entry)| (key.key(), entry.clone()))
                .collect::<Vec<_>>(),
            reparsed
                .entries()
                .map(|(key, entry)| (key.key(), entry.clone()))
                .collect::<Vec<_>>()
        );
        assert_eq!(rendered, reparsed.render());
        assert!(reparsed.is_canonical(&rendered));
    }
});

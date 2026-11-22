//! The command sequences in the README, the quickstart, and the evolution guide, run as written.
//!
//! Each test walks one document in order: a `wit` or `toml` block is saved where the surrounding
//! text says, and an `sh` block is executed line by line against the built binary. The blocks
//! are not handed to a shell. A small interpreter understands the handful of commands the
//! documents use (`witforge`, `mkdir`, `cp`, `echo … > file`) and refuses anything else, so the
//! sequences stay runnable on every platform and a new kind of command needs a deliberate
//! change here.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn repository() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn document(path: &str) -> String {
    fs::read_to_string(repository().join(path)).unwrap()
}

fn scratch(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("witforge-docs-{name}-{}", std::process::id()));
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).unwrap();
    root
}

/// Every fenced block of a Markdown document, in order, as `(language, body)`.
fn fenced_blocks(markdown: &str) -> Vec<(String, String)> {
    let mut blocks = Vec::new();
    let mut current: Option<(String, String)> = None;
    for line in markdown.lines() {
        if let Some(info) = line.strip_prefix("```") {
            match current.take() {
                Some(block) => blocks.push(block),
                None => current = Some((info.trim().to_owned(), String::new())),
            }
            continue;
        }
        if let Some((_, body)) = &mut current {
            body.push_str(line);
            body.push('\n');
        }
    }
    assert!(current.is_none(), "unterminated fenced block");
    blocks
}

/// One documented command: its words, and the exit status its trailing comment promises.
#[derive(Debug)]
struct Line {
    words: Vec<String>,
    expected: i32,
    text: String,
}

/// Splits an `sh` block into commands and standalone comments, joining `\` continuations.
fn parse_block(block: &str) -> Vec<Result<Line, String>> {
    let mut items = Vec::new();
    let mut pending = String::new();
    for raw in block.lines() {
        let line = raw.trim();
        if pending.is_empty() {
            if line.is_empty() {
                continue;
            }
            if let Some(comment) = line.strip_prefix('#') {
                items.push(Err(comment.trim().to_owned()));
                continue;
            }
        }
        if let Some(head) = line.strip_suffix('\\') {
            pending.push_str(head);
            pending.push(' ');
            continue;
        }
        pending.push_str(line);
        let text = std::mem::take(&mut pending);

        // A trailing `# exits N` states the documented exit status; any other trailing comment
        // is a remark.
        let (command, comment) = match text.split_once(" #") {
            Some((command, comment)) => (command, comment.trim()),
            None => (text.as_str(), ""),
        };
        let expected = comment.strip_prefix("exits ").map_or(0, |rest| {
            let digits: String = rest.chars().take_while(char::is_ascii_digit).collect();
            digits
                .parse()
                .unwrap_or_else(|_| panic!("`{text}`: `exits` needs a status"))
        });
        assert!(
            !command.contains(['|', '&', ';', '$', '"', '\'', '`', '*', '<', '(']),
            "`{text}` uses shell syntax the documentation test does not interpret"
        );
        let words = command.split_whitespace().map(str::to_owned).collect();
        items.push(Ok(Line {
            words,
            expected,
            text: text.clone(),
        }));
    }
    assert_eq!(pending.len(), 0, "a block ends in a line continuation");
    items
}

/// Runs one `sh` block in `directory`. `on_comment` sees every standalone comment line, which
/// is where a document tells the reader to edit a file. Returns how many `witforge` commands
/// ran.
fn run_block(directory: &Path, block: &str, on_comment: &mut dyn FnMut(&str)) -> usize {
    let mut ran = 0;
    for item in parse_block(block) {
        let line = match item {
            Ok(line) => line,
            Err(comment) => {
                on_comment(&comment);
                continue;
            }
        };
        let words: Vec<&str> = line.words.iter().map(String::as_str).collect();
        match words.as_slice() {
            // Installing the release is the one step the test cannot and need not repeat.
            ["cargo", "install", "witforge-cli", ..] => {}
            ["cargo", "run", "-p", "witforge-cli", "--", arguments @ ..]
            | ["witforge", arguments @ ..] => {
                let output = Command::new(env!("CARGO_BIN_EXE_witforge"))
                    .current_dir(directory)
                    .args(arguments)
                    .output()
                    .unwrap();
                assert_eq!(
                    output.status.code(),
                    Some(line.expected),
                    "`{}` exited differently than documented\nstdout: {}\nstderr: {}",
                    line.text,
                    String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr)
                );
                ran += 1;
            }
            ["mkdir", "-p", directories @ ..] if !directories.is_empty() => {
                for name in directories {
                    fs::create_dir_all(directory.join(name)).unwrap();
                }
            }
            ["mkdir", directories @ ..] if !directories.is_empty() => {
                for name in directories {
                    fs::create_dir(directory.join(name))
                        .unwrap_or_else(|error| panic!("`{}`: {error}", line.text));
                }
            }
            ["cp", sources @ .., target] if !sources.is_empty() => {
                let target_path = directory.join(target);
                let into_directory = target.ends_with('/') || target_path.is_dir();
                assert!(
                    into_directory || sources.len() == 1,
                    "`{}` copies several files to one file",
                    line.text
                );
                for source in sources {
                    let from = directory.join(source);
                    let to = if into_directory {
                        target_path.join(from.file_name().unwrap())
                    } else {
                        target_path.clone()
                    };
                    fs::copy(&from, &to).unwrap_or_else(|error| panic!("`{}`: {error}", line.text));
                }
            }
            ["echo", text @ .., ">", file] => {
                fs::write(directory.join(file), format!("{}\n", text.join(" ")))
                    .unwrap_or_else(|error| panic!("`{}`: {error}", line.text));
            }
            _ => panic!(
                "`{}` is not a command the documentation test interprets",
                line.text
            ),
        }
    }
    ran
}

fn no_comments(comment: &str) {
    panic!("unexpected instruction in a command block: `# {comment}`");
}

/// What a reader does at "Edit wit/v0.2/contract.wit: set the package version to 0.2.0, then
/// change the contract": both halves of the instruction. The change is the one the guides
/// recommend, a defaultable field appended to the first record, so the commands after the edit
/// see a real difference between the versions and `diff --check` reports it.
fn edit_copied_contract(directory: &Path, comment: &str, edits: &mut usize) {
    assert!(
        comment.starts_with(
            "Edit wit/v0.2/contract.wit: set the package version to 0.2.0, then change the contract"
        ),
        "unexpected instruction in a command block: `# {comment}`"
    );
    let path = directory.join("wit/v0.2/contract.wit");
    let copied = fs::read_to_string(&path).unwrap();
    assert!(
        copied.contains("@0.1.0;"),
        "the copied contract is not 0.1.0"
    );
    let bumped = copied.replacen("@0.1.0;", "@0.2.0;", 1);

    // A record body holds no braces, so the first `}` after the declaration closes it.
    let mut offset = 0;
    let mut declaration = None;
    for line in bumped.split_inclusive('\n') {
        if line.trim_start().starts_with("record ") {
            declaration = Some(offset);
            break;
        }
        offset += line.len();
    }
    let declaration = declaration.expect("the copied contract has a record");
    let open = declaration + bumped[declaration..].find('{').unwrap();
    let close = open + bumped[open..].find('}').unwrap();
    let fields = bumped[open + 1..close].trim_end();
    let separator = if fields.ends_with(',') { "" } else { "," };
    let edited = format!(
        "{}{fields}{separator}\n    added-in-v2: option<string>,\n  {}",
        &bumped[..=open],
        &bumped[close..]
    );
    fs::write(&path, edited).unwrap();
    *edits += 1;
}

#[test]
fn quickstart_commands_run_verbatim() {
    let quickstart = document("docs/quickstart.md");
    // The walkthrough is one sequence. The reference sections after it show fragments.
    let (walkthrough, reference) = quickstart
        .split_once("\n## Non-Rust consumers")
        .expect("the quickstart has a `Non-Rust consumers` section after the walkthrough");
    assert!(
        reference.contains("\n## Supported WIT subset"),
        "the walkthrough boundary moved"
    );

    let directory = scratch("quickstart");
    let mut contracts = [
        "my-contract/wit/v0.1/contract.wit",
        "my-contract/wit/v0.2/contract.wit",
    ]
    .into_iter();
    let mut configurations = 0;
    let mut ran = 0;
    for (language, body) in fenced_blocks(walkthrough) {
        match language.as_str() {
            // The directory must already exist: the page has to say how to create it.
            "wit" => {
                let target = contracts
                    .next()
                    .expect("a third WIT block in the walkthrough");
                fs::write(directory.join(target), body).unwrap_or_else(|error| {
                    panic!("the page never created the directory of {target}: {error}")
                });
            }
            "toml" => {
                fs::write(directory.join("my-contract/witforge.toml"), body).unwrap();
                configurations += 1;
            }
            "sh" => ran += run_block(&directory, &body, &mut no_comments),
            "text" => {}
            other => panic!("unexpected `{other}` block in the quickstart walkthrough"),
        }
    }
    assert!(
        contracts.next().is_none(),
        "the walkthrough lost a WIT block"
    );
    assert_eq!(configurations, 1, "the walkthrough has one configuration");
    assert!(ran >= 20, "only {ran} witforge commands ran");

    let contract = directory.join("my-contract");
    for generated in [
        "wit/v0.1/wire.lock",
        "wit/v0.2/wire.lock",
        "generated/rust/v0_1.rs",
        "generated/rust/v0_2.rs",
        "generated/rust/convert.rs",
        "generated/proto/v0.proto",
        "generated/schema/v0_2.schema.json",
        "generated/glue/v0.rs",
    ] {
        assert!(
            contract.join(generated).is_file(),
            "the walkthrough did not produce {generated}"
        );
    }
    assert_eq!(
        fs::read_to_string(contract.join("wit/released")).unwrap(),
        "v0.1\n"
    );

    fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn readme_commands_run_verbatim() {
    let readme = document("README.md");
    let directory = scratch("readme");
    let mut contracts = 0;
    let mut edits = 0;
    let mut ran = 0;
    for (language, body) in fenced_blocks(&readme) {
        match language.as_str() {
            // "Create `wit/v0.1/contract.wit`".
            "wit" => {
                fs::create_dir_all(directory.join("wit/v0.1")).unwrap();
                fs::write(directory.join("wit/v0.1/contract.wit"), body).unwrap();
                contracts += 1;
            }
            "sh" => {
                ran += run_block(&directory, &body, &mut |comment| {
                    edit_copied_contract(&directory, comment, &mut edits);
                });
            }
            // Output excerpts are compared with emitter output in `distribution.rs`; the
            // workflow snippet is not a local command.
            "rust" | "proto" | "yaml" => {}
            other => panic!("unexpected `{other}` block in the README"),
        }
    }
    assert_eq!(contracts, 1, "the README has one contract");
    assert_eq!(
        edits, 1,
        "the README's evolution sequence has one edit step"
    );
    assert!(ran >= 9, "only {ran} witforge commands ran");
    for generated in [
        "generated/rust/v0_1.rs",
        "generated/proto/v0.proto",
        "generated/schema/v0_1.schema.json",
        "generated/glue/v0.rs",
        "wit/v0.2/wire.lock",
    ] {
        assert!(
            directory.join(generated).is_file(),
            "the README did not produce {generated}"
        );
    }

    fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn evolution_guide_commands_run_verbatim() {
    let guide = document("docs/guide/evolution.md");
    let directory = scratch("evolution");
    // The guide starts from a locked first version; the single-version example is one.
    let first = directory.join("wit/v0.1");
    fs::create_dir_all(&first).unwrap();
    let example = repository().join("examples/single/wit/v0.1");
    for name in ["contract.wit", "wire.lock"] {
        fs::copy(example.join(name), first.join(name)).unwrap();
    }

    let mut edits = 0;
    let mut major_edits = 0;
    let mut ran = 0;
    for (language, body) in fenced_blocks(&guide) {
        match language.as_str() {
            "sh" => {
                ran += run_block(&directory, &body, &mut |comment| {
                    if comment.starts_with("Edit wit/v1.0/contract.wit") {
                        let path = directory.join("wit/v1.0/contract.wit");
                        let text = fs::read_to_string(&path).unwrap();
                        assert!(text.contains("@1.0.0;"), "v1.0 contract was not bumped");
                        let open = text.find("record ").unwrap();
                        let brace = open + text[open..].find('{').unwrap();
                        let close = brace + text[brace..].find('}').unwrap();
                        let fields = text[brace + 1..close].trim_end();
                        let sep = if fields.ends_with(',') { "" } else { "," };
                        let edited = format!(
                            "{}{fields}{sep}\n    required-in-v1: u64,\n  {}",
                            &text[..=brace],
                            &text[close..]
                        );
                        fs::write(&path, edited).unwrap();
                        major_edits += 1;
                    } else {
                        edit_copied_contract(&directory, comment, &mut edits);
                    }
                });
            }
            "text" => {}
            other => panic!("unexpected `{other}` block in the evolution guide"),
        }
    }
    assert_eq!(edits, 1, "the guide's minor sequence has one edit step");
    assert_eq!(
        major_edits, 1,
        "the guide's major sequence has one edit step"
    );
    assert_eq!(
        ran, 9,
        "the guide runs minor lock/check/diff/diff --check and major new-version/lock/check/diff/rust"
    );
    let lock = fs::read_to_string(directory.join("wit/v0.2/wire.lock")).unwrap();
    assert!(
        lock.contains("example:ingest/types/item.added-in-v2 = 5 field option<string>"),
        "the edit step did not change the contract:\n{lock}"
    );
    assert!(directory.join("generated/rust/v1_0.rs").is_file());
    let convert = fs::read_to_string(directory.join("generated/rust/convert.rs")).unwrap();
    assert!(
        convert.contains("required-in-v1"),
        "expected cross-major omission comment in convert.rs:\n{convert}"
    );

    fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn the_documented_sequence_fails_without_its_edit_step() {
    // The step the guides used to omit: with the copied `@0.1.0` left in place, the next
    // command refuses the new directory.
    let directory = scratch("evolution-unedited");
    let first = directory.join("wit/v0.1");
    fs::create_dir_all(&first).unwrap();
    let example = repository().join("examples/single/wit/v0.1");
    for name in ["contract.wit", "wire.lock"] {
        fs::copy(example.join(name), first.join(name)).unwrap();
    }
    run_block(
        &directory,
        "mkdir wit/v0.2\n\
         cp wit/v0.1/contract.wit wit/v0.1/wire.lock wit/v0.2/\n\
         witforge lock wit/v0.2 --against wit/v0.1/wire.lock   # exits 1\n",
        &mut no_comments,
    );
    fs::remove_dir_all(&directory).unwrap();
}

#[test]
fn the_interpreter_refuses_what_it_does_not_understand() {
    let directory = scratch("interpreter");
    for block in [
        "rm -rf wit\n",
        "witforge check wit/v0.1 && witforge diff a b\n",
        "cat wit/v0.1/wire.lock | head\n",
    ] {
        let attempt = std::panic::catch_unwind(|| {
            run_block(&directory, block, &mut no_comments);
        });
        assert!(attempt.is_err(), "`{}` was accepted", block.trim());
    }
    let parsed =
        parse_block("witforge diff a b --check   # exits 1: breaking\nwitforge \\\n  check a\n");
    let lines: Vec<&Line> = parsed.iter().map(|item| item.as_ref().unwrap()).collect();
    assert_eq!(lines[0].expected, 1);
    assert_eq!(lines[0].words, ["witforge", "diff", "a", "b", "--check"]);
    assert_eq!(lines[1].expected, 0);
    assert_eq!(lines[1].words, ["witforge", "check", "a"]);
    fs::remove_dir_all(&directory).unwrap();
}

#[allow(clippy::too_many_lines)]
#[test]
fn wiring_guide_crate_generates_compiles_and_roundtrips() {
    let guide = document("docs/guide/wiring.md");
    let (_, after_heading) = guide
        .split_once("\n## Empty crate to compiling round trip")
        .expect("wiring guide has `Empty crate to compiling round trip` section");
    let (walkthrough, _) = after_heading
        .split_once("\n## Module roots, dependencies")
        .expect("wiring guide has `Module roots, dependencies` section after walkthrough");

    let directory = scratch("wiring");
    let mut toml_targets = [
        "my-contract-crate/Cargo.toml",
        "my-contract-crate/witforge.toml",
    ]
    .into_iter();
    let mut rust_targets = [
        "my-contract-crate/build.rs",
        "my-contract-crate/src/lib.rs",
        "my-contract-crate/src/main.rs",
    ]
    .into_iter();
    let mut wit_count = 0usize;
    let mut witforge_ran = 0usize;
    let mut cargo_ran = 0usize;

    // Share one per-rustc offline build cache under TMPDIR so prost/protox compile once.
    let rustc_ver = Command::new("rustc").arg("-V").output().map_or_else(
        |_| "rustc".to_string(),
        |o| {
            String::from_utf8_lossy(&o.stdout)
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
                .collect::<String>()
        },
    );
    let shared_target = std::env::temp_dir().join(format!("wf-wiring-t-{rustc_ver}"));

    for (language, body) in fenced_blocks(walkthrough) {
        match language.as_str() {
            "text" => {}
            "toml" => {
                let target = toml_targets
                    .next()
                    .expect("unexpected extra toml block in wiring walkthrough");
                fs::write(directory.join(target), body).unwrap();
            }
            "wit" => {
                fs::write(
                    directory.join("my-contract-crate/wit/v0.1/contract.wit"),
                    body,
                )
                .unwrap();
                wit_count += 1;
            }
            "rust" => {
                let target = rust_targets
                    .next()
                    .expect("unexpected extra rust block in wiring walkthrough");
                fs::write(directory.join(target), body).unwrap();
            }
            "sh" => {
                let trimmed = body.trim();
                if trimmed == "cargo run --manifest-path my-contract-crate/Cargo.toml" {
                    // Copy the workspace lockfile so the guide crate compiles offline with the
                    // repository's locked prost 0.14 / protox 0.10 dependencies.
                    let crate_dir = directory.join("my-contract-crate");
                    fs::copy(
                        repository().join("Cargo.lock"),
                        crate_dir.join("Cargo.lock"),
                    )
                    .unwrap();
                    let clippy = Command::new(env!("CARGO"))
                        .args(["clippy", "--offline", "--all-targets", "--manifest-path"])
                        .arg(crate_dir.join("Cargo.toml"))
                        .args(["--", "-D", "warnings"])
                        .env("CARGO_TARGET_DIR", &shared_target)
                        .env("CARGO_TERM_COLOR", "never")
                        .env_remove("RUSTFLAGS")
                        .output()
                        .unwrap();
                    assert!(
                        clippy.status.success(),
                        "wiring guide crate failed clippy:\nstdout:\n{}\nstderr:\n{}",
                        String::from_utf8_lossy(&clippy.stdout),
                        String::from_utf8_lossy(&clippy.stderr)
                    );
                    let run = Command::new(env!("CARGO"))
                        .args(["run", "--offline", "--manifest-path"])
                        .arg(crate_dir.join("Cargo.toml"))
                        .env("CARGO_TARGET_DIR", &shared_target)
                        .env("CARGO_TERM_COLOR", "never")
                        .env_remove("RUSTFLAGS")
                        .output()
                        .unwrap();
                    assert!(
                        run.status.success(),
                        "wiring guide crate failed cargo run:\nstdout:\n{}\nstderr:\n{}",
                        String::from_utf8_lossy(&run.stdout),
                        String::from_utf8_lossy(&run.stderr)
                    );
                    cargo_ran += 1;
                } else {
                    witforge_ran += run_block(&directory, &body, &mut no_comments);
                }
            }
            other => panic!("unexpected `{other}` block in wiring guide walkthrough"),
        }
    }

    assert!(
        toml_targets.next().is_none(),
        "missing toml block in wiring guide"
    );
    assert!(
        rust_targets.next().is_none(),
        "missing rust block in wiring guide"
    );
    assert_eq!(wit_count, 1);
    assert_eq!(witforge_ran, 4);
    assert_eq!(cargo_ran, 1);
    fs::remove_dir_all(&directory).unwrap();
}

/// Compute GitHub Flavored Markdown heading anchors (matching GitHub and kramdown GFM slugs,
/// including `-1`, `-2` suffixes for duplicate headings).
fn markdown_anchors(markdown: &str) -> std::collections::BTreeSet<String> {
    let mut anchors = std::collections::BTreeSet::new();
    let mut counts: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let mut in_fence = false;
    for line in markdown.lines() {
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let trimmed = line.trim_start();
        if !trimmed.starts_with('#') {
            continue;
        }
        let hashes = trimmed.bytes().take_while(|&b| b == b'#').count();
        if !(1..=6).contains(&hashes) {
            continue;
        }
        let rest = &trimmed[hashes..];
        if !rest.starts_with(' ') {
            continue;
        }
        let title = rest.trim().trim_end_matches('#').trim();
        let slug = gfm_slug(title);
        if slug.is_empty() {
            continue;
        }
        let count = counts.entry(slug.clone()).or_insert(0);
        if *count == 0 {
            anchors.insert(slug);
        } else {
            anchors.insert(format!("{slug}-{count}"));
        }
        *count += 1;
    }
    anchors
}

fn gfm_slug(title: &str) -> String {
    let mut out = String::new();
    for ch in title.chars() {
        if ch.is_alphanumeric() {
            for lower in ch.to_lowercase() {
                out.push(lower);
            }
        } else if ch == ' ' || ch == '-' {
            out.push('-');
        } else if ch == '_' {
            out.push('_');
        }
    }
    out
}

/// Extract Markdown link targets `[label](target)` outside fenced code blocks and inline code.
fn extract_markdown_links(markdown: &str) -> Vec<(usize, String)> {
    let mut links = Vec::new();
    let mut in_fence = false;
    for (idx, line) in markdown.lines().enumerate() {
        let line_no = idx + 1;
        if line.trim_start().starts_with("```") {
            in_fence = !in_fence;
            continue;
        }
        if in_fence {
            continue;
        }
        let bytes = line.as_bytes();
        let mut i = 0;
        let mut in_code = false;
        while i < bytes.len() {
            if bytes[i] == b'`' {
                in_code = !in_code;
                i += 1;
                continue;
            }
            if !in_code && bytes[i] == b']' && i + 1 < bytes.len() && bytes[i + 1] == b'(' {
                let start = i + 2;
                let mut depth = 1usize;
                let mut j = start;
                while j < bytes.len() && depth > 0 {
                    match bytes[j] {
                        b'(' => depth += 1,
                        b')' => depth -= 1,
                        _ => {}
                    }
                    if depth > 0 {
                        j += 1;
                    }
                }
                if depth == 0 {
                    let raw = line[start..j].trim();
                    let target = raw
                        .split_whitespace()
                        .next()
                        .unwrap_or("")
                        .trim_matches(|c| c == '<' || c == '>');
                    if !target.is_empty() {
                        links.push((line_no, target.to_string()));
                    }
                    i = j + 1;
                    continue;
                }
            }
            i += 1;
        }
    }
    links
}

fn collect_markdown_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<PathBuf> = fs::read_dir(dir)
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .collect();
    entries.sort();
    for path in entries {
        if path.is_dir() {
            collect_markdown_files(&path, out);
        } else if path.extension().and_then(std::ffi::OsStr::to_str) == Some("md") {
            out.push(path);
        }
    }
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                out.pop();
            }
            other => out.push(other),
        }
    }
    out
}

fn verify_anchor(
    source_label: &str,
    line_no: usize,
    raw_target: &str,
    target_file: &Path,
    anchor: &str,
    anchor_cache: &mut std::collections::BTreeMap<PathBuf, std::collections::BTreeSet<String>>,
) {
    let anchors = anchor_cache
        .entry(target_file.to_path_buf())
        .or_insert_with(|| {
            let text = fs::read_to_string(target_file).unwrap_or_else(|err| {
                panic!("{source_label}:{line_no}: failed to read `{raw_target}`: {err}")
            });
            markdown_anchors(&text)
        });
    // Also accept kramdown's collapsed-dash anchor form (e.g. `060-2026-11-21` vs `060---2026-11-21`).
    let collapsed = anchor.replace("---", "-");
    let found =
        anchors.contains(anchor) || anchors.iter().any(|a| a.replace("---", "-") == collapsed);
    assert!(
        found,
        "{source_label}:{line_no}: broken anchor `#{anchor}` in `{}` (from `{raw_target}`)",
        target_file.display()
    );
}

#[allow(clippy::too_many_lines)]
#[test]
fn documentation_links_and_anchors_resolve_on_github_and_pages() {
    let repo = normalize_path(&repository());
    let docs_root = repo.join("docs");
    let config = fs::read_to_string(docs_root.join("_config.yml")).unwrap();
    assert!(
        config.contains("jekyll-relative-links"),
        "docs/_config.yml must enable jekyll-relative-links so .md links rewrite to .html on Pages"
    );

    let mut md_files = Vec::new();
    for top in [
        "README.md",
        "CONTRIBUTING.md",
        "SECURITY.md",
        "ROADMAP.md",
        "ci/README.md",
        "fuzz/README.md",
    ] {
        let path = repo.join(top);
        if path.is_file() {
            md_files.push(path);
        }
    }
    collect_markdown_files(&docs_root, &mut md_files);

    // Exclusions declared in `docs/_config.yml`: these files are NOT published on GitHub Pages.
    let is_pages_published = |path: &Path| -> bool {
        let Ok(rel) = path.strip_prefix(&docs_root) else {
            return false;
        };
        let first = rel
            .components()
            .next()
            .and_then(|c| c.as_os_str().to_str())
            .unwrap_or("");
        first != "design" && first != "aplan-pilot.md"
    };

    let mut anchor_cache = std::collections::BTreeMap::new();
    let pages_prefix = "https://asamarts.github.io/witforge/";
    let github_blob = "https://github.com/asamarts/witforge/blob/main/";
    let github_tree = "https://github.com/asamarts/witforge/tree/main/";

    for file in &md_files {
        let rel_source = file.strip_prefix(&repo).unwrap().display().to_string();
        let text = fs::read_to_string(file).unwrap();
        let in_docs = file.starts_with(&docs_root);
        let published_on_pages = is_pages_published(file);

        for (line_no, raw_target) in extract_markdown_links(&text) {
            if let Some(rest) = raw_target.strip_prefix(pages_prefix) {
                // Built GitHub Pages absolute URL (e.g. from README.md): verify it maps to an
                // existing published `.md` page under `docs/` and valid anchor!
                let (page_path, anchor) = match rest.split_once('#') {
                    Some((p, a)) => (p, Some(a)),
                    None => (rest, None),
                };
                let mapped = if page_path.is_empty() {
                    docs_root.join("index.md")
                } else if page_path.ends_with('/') {
                    docs_root.join(page_path).join("index.md")
                } else if let Some(stem) = page_path.strip_suffix(".html") {
                    docs_root.join(format!("{stem}.md"))
                } else {
                    docs_root.join(page_path)
                };
                let mapped = normalize_path(&mapped);
                assert!(
                    mapped.is_file() && is_pages_published(&mapped),
                    "{rel_source}:{line_no}: Pages URL `{raw_target}` does not map to a published page ({})",
                    mapped.display()
                );
                if let Some(anchor) = anchor {
                    verify_anchor(
                        &rel_source,
                        line_no,
                        &raw_target,
                        &mapped,
                        anchor,
                        &mut anchor_cache,
                    );
                }
                continue;
            }
            if let Some(rest) = raw_target
                .strip_prefix(github_blob)
                .or_else(|| raw_target.strip_prefix(github_tree))
            {
                let (repo_path, anchor) = match rest.split_once('#') {
                    Some((p, a)) => (p, Some(a)),
                    None => (rest, None),
                };
                let target = normalize_path(&repo.join(repo_path));
                assert!(
                    target.exists(),
                    "{rel_source}:{line_no}: GitHub URL `{raw_target}` points to missing path {}",
                    target.display()
                );
                if let Some(anchor) = anchor
                    && target.extension().and_then(std::ffi::OsStr::to_str) == Some("md")
                    && !anchor.starts_with('L')
                {
                    verify_anchor(
                        &rel_source,
                        line_no,
                        &raw_target,
                        &target,
                        anchor,
                        &mut anchor_cache,
                    );
                }
                continue;
            }
            if raw_target.starts_with("http://")
                || raw_target.starts_with("https://")
                || raw_target.starts_with("mailto:")
            {
                continue;
            }

            // Relative link or same-page `#anchor`:
            let (rel_path, anchor) = match raw_target.split_once('#') {
                Some((p, a)) => (p, Some(a)),
                None => (raw_target.as_str(), None),
            };
            if rel_path.is_empty() {
                if let Some(anchor) = anchor {
                    verify_anchor(
                        &rel_source,
                        line_no,
                        &raw_target,
                        file,
                        anchor,
                        &mut anchor_cache,
                    );
                }
                continue;
            }

            // 1. GitHub repository check: no relative `.html` targets in `docs/`, and the file or
            //    directory must exist on disk.
            if in_docs {
                assert!(
                    !Path::new(rel_path)
                        .extension()
                        .is_some_and(|ext| ext.eq_ignore_ascii_case("html")),
                    "{rel_source}:{line_no}: relative `.html` link `{raw_target}` breaks on GitHub; use `.md`"
                );
            }
            let target = normalize_path(&file.parent().unwrap().join(rel_path));
            assert!(
                target.exists(),
                "{rel_source}:{line_no}: broken relative link `{raw_target}` (resolved to {})",
                target.display()
            );

            // 2. Built GitHub Pages site check: any relative link from a published page MUST stay
            //    inside the published `docs/` tree (not `docs/design/`, `docs/aplan-pilot.md`, or
            //    outside `docs/`), where `jekyll-relative-links` rewrites `.md` -> `.html`.
            if published_on_pages {
                assert!(
                    is_pages_published(&target),
                    "{rel_source}:{line_no}: published page links relatively to excluded path `{raw_target}`; link to GitHub instead"
                );
            }

            if let Some(anchor) = anchor
                && target.extension().and_then(std::ffi::OsStr::to_str) == Some("md")
            {
                verify_anchor(
                    &rel_source,
                    line_no,
                    &raw_target,
                    &target,
                    anchor,
                    &mut anchor_cache,
                );
            }
        }
    }
}

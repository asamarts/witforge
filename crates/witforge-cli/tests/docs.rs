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
    assert!(pending.is_empty(), "a block ends in a line continuation");
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

/// What a reader does at "Edit wit/v0.2/contract.wit: set the package version to 0.2.0": the
/// smallest edit that makes the copied contract a valid `v0.2`.
fn bump_copied_version(directory: &Path, comment: &str, edits: &mut usize) {
    assert!(
        comment.starts_with("Edit wit/v0.2/contract.wit: set the package version to 0.2.0"),
        "unexpected instruction in a command block: `# {comment}`"
    );
    let path = directory.join("wit/v0.2/contract.wit");
    let copied = fs::read_to_string(&path).unwrap();
    assert!(
        copied.contains("@0.1.0;"),
        "the copied contract is not 0.1.0"
    );
    fs::write(&path, copied.replacen("@0.1.0;", "@0.2.0;", 1)).unwrap();
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
                    bump_copied_version(&directory, comment, &mut edits);
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
    let mut ran = 0;
    for (language, body) in fenced_blocks(&guide) {
        assert_eq!(language, "sh", "unexpected block in the evolution guide");
        ran += run_block(&directory, &body, &mut |comment| {
            bump_copied_version(&directory, comment, &mut edits);
        });
    }
    assert_eq!(edits, 1, "the guide's sequence has one edit step");
    assert_eq!(ran, 4, "the guide runs lock, check, diff, and diff --check");
    assert!(directory.join("wit/v0.2/wire.lock").is_file());

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

//! Shell completion generation derived directly from `clap::Command` definitions.
//!
//! Introspecting the compiled `Cli::command()` keeps bash, zsh, and fish completions in sync with
//! every subcommand, option, and enum value without an external generator dependency.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use clap::{Arg, Command, ValueEnum};

/// Target shell for `witforge completions`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum Shell {
    /// GNU Bash completion script.
    Bash,
    /// Zsh `#compdef` completion script.
    Zsh,
    /// Fish `complete` completion script.
    Fish,
}

/// Generate a completion script for `shell` from `cmd`.
pub(crate) fn generate(shell: Shell, cmd: &mut Command) -> String {
    cmd.build();
    match shell {
        Shell::Bash => generate_bash(cmd),
        Shell::Zsh => generate_zsh(cmd),
        Shell::Fish => generate_fish(cmd),
    }
}

fn visible_subcommands(cmd: &Command) -> Vec<&Command> {
    cmd.get_subcommands()
        .filter(|sub| !sub.is_hide_set())
        .collect()
}

fn visible_opts(cmd: &Command) -> Vec<&Arg> {
    cmd.get_arguments()
        .filter(|arg| !arg.is_hide_set() && !arg.is_positional())
        .collect()
}

fn visible_positionals(cmd: &Command) -> Vec<&Arg> {
    cmd.get_arguments()
        .filter(|arg| !arg.is_hide_set() && arg.is_positional())
        .collect()
}

fn arg_tokens(arg: &Arg) -> Vec<String> {
    let mut tokens = Vec::new();
    if let Some(long) = arg.get_long() {
        tokens.push(format!("--{long}"));
    }
    if let Some(short) = arg.get_short() {
        tokens.push(format!("-{short}"));
    }
    tokens
}

fn possible_values(arg: &Arg) -> Vec<String> {
    arg.get_possible_values()
        .into_iter()
        .filter(|val| !val.is_hide_set())
        .map(|val| val.get_name().to_string())
        .collect()
}

fn first_line_help(text: Option<&clap::builder::StyledStr>) -> String {
    text.map(ToString::to_string)
        .unwrap_or_default()
        .lines()
        .next()
        .unwrap_or_default()
        .trim()
        .to_string()
}

#[allow(clippy::too_many_lines)]
fn generate_bash(cmd: &Command) -> String {
    let bin = cmd.get_name();
    let subcommands = visible_subcommands(cmd);
    let sub_names: Vec<&str> = subcommands.iter().map(|sub| sub.get_name()).collect();

    // Global flags that consume the next separate word so subcommand scanning skips their values.
    let mut global_value_flags = BTreeSet::new();
    let mut flag_value_cases: Vec<(String, String)> = Vec::new();
    let mut seen_flag_values = BTreeSet::new();
    let mut root_flags = Vec::new();
    for arg in visible_opts(cmd) {
        let tokens = arg_tokens(arg);
        let takes_val = arg.get_action().takes_values() && !arg.is_require_equals_set();
        let vals = possible_values(arg);
        for tok in tokens {
            if arg.is_global_set() && takes_val {
                global_value_flags.insert(tok.clone());
            }
            if !vals.is_empty() && seen_flag_values.insert(tok.clone()) {
                flag_value_cases.push((tok.clone(), vals.join(" ")));
            }
            root_flags.push(tok);
        }
    }
    for sub in &subcommands {
        for arg in visible_opts(sub) {
            let vals = possible_values(arg);
            if vals.is_empty() {
                continue;
            }
            for tok in arg_tokens(arg) {
                if seen_flag_values.insert(tok.clone()) {
                    flag_value_cases.push((tok, vals.join(" ")));
                }
            }
        }
    }

    let mut out = String::new();
    let _ = writeln!(out, "# Bash completion for {bin}.");
    let _ = writeln!(out, "_{bin}() {{");
    let _ = writeln!(out, "    local cur prev subcmd i word skip_next");
    let _ = writeln!(out, "    COMPREPLY=()");
    let _ = writeln!(out, "    cur=\"${{COMP_WORDS[COMP_CWORD]}}\"");
    let _ = writeln!(out, "    prev=\"\"");
    let _ = writeln!(out, "    if (( COMP_CWORD > 0 )); then");
    let _ = writeln!(out, "        prev=\"${{COMP_WORDS[COMP_CWORD-1]}}\"");
    let _ = writeln!(out, "    fi");
    let _ = writeln!(out, "    subcmd=\"\"");
    let _ = writeln!(out, "    skip_next=0");
    let _ = writeln!(out, "    for (( i=1; i<COMP_CWORD; i++ )); do");
    let _ = writeln!(out, "        word=\"${{COMP_WORDS[i]}}\"");
    let _ = writeln!(out, "        if (( skip_next )); then");
    let _ = writeln!(out, "            skip_next=0");
    let _ = writeln!(out, "            continue");
    let _ = writeln!(out, "        fi");
    let _ = writeln!(out, "        case \"$word\" in");
    if !global_value_flags.is_empty() {
        let pattern = global_value_flags
            .iter()
            .cloned()
            .collect::<Vec<_>>()
            .join("|");
        let _ = writeln!(out, "            {pattern})");
        let _ = writeln!(out, "                skip_next=1");
        let _ = writeln!(out, "                ;;");
    }
    let _ = writeln!(out, "            -*)");
    let _ = writeln!(out, "                ;;");
    let _ = writeln!(out, "            *)");
    let _ = writeln!(out, "                subcmd=\"$word\"");
    let _ = writeln!(out, "                break");
    let _ = writeln!(out, "                ;;");
    let _ = writeln!(out, "        esac");
    let _ = writeln!(out, "    done");

    if !flag_value_cases.is_empty() {
        let _ = writeln!(out, "    case \"$prev\" in");
        for (flag, vals) in &flag_value_cases {
            let _ = writeln!(out, "        {flag})");
            let _ = writeln!(
                out,
                "            COMPREPLY=( $(compgen -W \"{vals}\" -- \"$cur\") )"
            );
            let _ = writeln!(out, "            return 0");
            let _ = writeln!(out, "            ;;");
        }
        let _ = writeln!(out, "    esac");
    }

    let _ = writeln!(out, "    if [[ -z \"$subcmd\" ]]; then");
    let _ = writeln!(out, "        if [[ \"$cur\" == -* ]]; then");
    let _ = writeln!(
        out,
        "            COMPREPLY=( $(compgen -W \"{}\" -- \"$cur\") )",
        root_flags.join(" ")
    );
    let _ = writeln!(out, "        else");
    let _ = writeln!(
        out,
        "            COMPREPLY=( $(compgen -W \"{}\" -- \"$cur\") )",
        sub_names.join(" ")
    );
    let _ = writeln!(out, "        fi");
    let _ = writeln!(out, "        return 0");
    let _ = writeln!(out, "    fi");

    let _ = writeln!(out, "    case \"$subcmd\" in");
    for sub in &subcommands {
        let mut flags = Vec::new();
        for arg in visible_opts(sub) {
            flags.extend(arg_tokens(arg));
        }
        let pos_vals: Vec<String> = visible_positionals(sub)
            .into_iter()
            .flat_map(possible_values)
            .collect();
        let _ = writeln!(out, "        {})", sub.get_name());
        let _ = writeln!(out, "            if [[ \"$cur\" == -* ]]; then");
        let _ = writeln!(
            out,
            "                COMPREPLY=( $(compgen -W \"{}\" -- \"$cur\") )",
            flags.join(" ")
        );
        if !pos_vals.is_empty() {
            let _ = writeln!(out, "            else");
            let _ = writeln!(
                out,
                "                COMPREPLY=( $(compgen -W \"{}\" -- \"$cur\") )",
                pos_vals.join(" ")
            );
        } else if sub.get_name() == "help" {
            let _ = writeln!(out, "            else");
            let _ = writeln!(
                out,
                "                COMPREPLY=( $(compgen -W \"{}\" -- \"$cur\") )",
                sub_names.join(" ")
            );
        }
        let _ = writeln!(out, "            fi");
        let _ = writeln!(out, "            ;;");
    }
    let _ = writeln!(out, "    esac");
    let _ = writeln!(out, "}}");
    let _ = writeln!(out, "complete -F _{bin} -o bashdefault -o default {bin}");
    out
}

fn zsh_escape(text: &str) -> String {
    text.replace('\\', "\\\\")
        .replace('\'', "'\\''")
        .replace('[', "\\[")
        .replace(']', "\\]")
        .replace(':', "\\:")
}

fn zsh_arg_spec(arg: &Arg) -> Vec<String> {
    let help = zsh_escape(&first_line_help(arg.get_help()));
    let takes_val = arg.get_action().takes_values();
    let vals = possible_values(arg);
    let value_suffix = if takes_val {
        let value_name = arg
            .get_value_names()
            .and_then(|names| names.first())
            .map_or("VALUE", clap::builder::Str::as_str);
        if !vals.is_empty() {
            format!(":{value_name}:({})", vals.join(" "))
        } else if matches!(value_name, "DIR" | "WIT_DIR" | "WIT_ROOT" | "OLD" | "NEW") {
            format!(":{value_name}:_files -/")
        } else if matches!(value_name, "PATH") {
            format!(":{value_name}:_files")
        } else {
            format!(":{value_name}: ")
        }
    } else {
        String::new()
    };

    let eq = if takes_val
        && arg.is_require_equals_set()
        && arg
            .get_num_args()
            .is_some_and(|range| range.min_values() == 0)
    {
        "=-"
    } else if takes_val {
        "="
    } else {
        ""
    };
    let mut specs = Vec::new();
    if let Some(long) = arg.get_long() {
        specs.push(format!("'--{long}{eq}[{help}]{value_suffix}'"));
    }
    if let Some(short) = arg.get_short() {
        specs.push(format!("'-{short}[{help}]{value_suffix}'"));
    }
    specs
}

fn generate_zsh(cmd: &Command) -> String {
    let bin = cmd.get_name();
    let subcommands = visible_subcommands(cmd);

    let mut out = String::new();
    let _ = writeln!(out, "#compdef {bin}");
    let _ = writeln!(out, "autoload -U is-at-least\n");

    let _ = writeln!(out, "_{bin}() {{");
    let _ = writeln!(out, "    local context state state_descr line");
    let _ = writeln!(out, "    typeset -A opt_args");
    let _ = writeln!(out, "    _arguments -C \\");
    for arg in visible_opts(cmd) {
        for spec in zsh_arg_spec(arg) {
            let _ = writeln!(out, "        {spec} \\");
        }
    }
    let _ = writeln!(out, "        '1: :_{bin}_commands' \\");
    let _ = writeln!(out, "        '*::arg:->args'");
    let _ = writeln!(out, "    case $state in");
    let _ = writeln!(out, "        args)");
    let _ = writeln!(out, "            case $line[1] in");
    for sub in &subcommands {
        let fn_suffix = sub.get_name().replace('-', "_");
        let _ = writeln!(
            out,
            "                {}) _{bin}_{fn_suffix} ;;",
            sub.get_name()
        );
    }
    let _ = writeln!(out, "            esac");
    let _ = writeln!(out, "            ;;");
    let _ = writeln!(out, "    esac");
    let _ = writeln!(out, "}}\n");

    let _ = writeln!(out, "_{bin}_commands() {{");
    let _ = writeln!(out, "    local commands; commands=(");
    for sub in &subcommands {
        let desc = zsh_escape(&first_line_help(sub.get_about()));
        let _ = writeln!(out, "        '{}:{desc}'", sub.get_name());
    }
    let _ = writeln!(out, "    )");
    let _ = writeln!(out, "    _describe -t commands '{bin} commands' commands");
    let _ = writeln!(out, "}}\n");

    for sub in &subcommands {
        let fn_suffix = sub.get_name().replace('-', "_");
        let _ = writeln!(out, "_{bin}_{fn_suffix}() {{");
        let mut specs = Vec::new();
        for arg in visible_opts(sub) {
            specs.extend(zsh_arg_spec(arg));
        }
        for (idx, pos) in visible_positionals(sub).into_iter().enumerate() {
            let ordinal = idx + 1;
            let name = pos
                .get_value_names()
                .and_then(|names| names.first())
                .map_or("ARG", clap::builder::Str::as_str);
            let vals = possible_values(pos);
            if !vals.is_empty() {
                specs.push(format!("'{ordinal}:{name}:({})'", vals.join(" ")));
            } else if matches!(name, "PATH" | "WIT_DIR" | "WIT_ROOT" | "OLD" | "NEW") {
                specs.push(format!("'{ordinal}:{name}:_files'"));
            } else {
                specs.push(format!("'{ordinal}:{name}: '"));
            }
        }
        if specs.is_empty() {
            let _ = writeln!(out, "    _arguments");
        } else {
            let _ = writeln!(out, "    _arguments \\");
            for (index, spec) in specs.iter().enumerate() {
                let tail = if index + 1 < specs.len() { " \\" } else { "" };
                let _ = writeln!(out, "        {spec}{tail}");
            }
        }
        let _ = writeln!(out, "}}\n");
    }

    let _ = writeln!(
        out,
        "if [ \"$funcstack[1]\" = \"_{bin}\" ]; then\n    _{bin} \"$@\"\nelse\n    compdef _{bin} {bin}\nfi"
    );
    out
}

fn fish_escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('\'', "\\'")
}

fn generate_fish(cmd: &Command) -> String {
    let bin = cmd.get_name();
    let subcommands = visible_subcommands(cmd);
    let mut out = String::new();
    let _ = writeln!(out, "# Fish completion for {bin}.");

    for arg in visible_opts(cmd) {
        write_fish_opt(&mut out, bin, "__fish_use_subcommand", arg);
    }

    for sub in &subcommands {
        let name = sub.get_name();
        let desc = fish_escape(&first_line_help(sub.get_about()));
        let _ = writeln!(
            out,
            "complete -c {bin} -n '__fish_use_subcommand' -f -a '{name}' -d '{desc}'"
        );
    }

    for sub in &subcommands {
        let name = sub.get_name();
        let cond = format!("__fish_seen_subcommand_from {name}");
        for arg in visible_opts(sub) {
            write_fish_opt(&mut out, bin, &cond, arg);
        }
        for pos in visible_positionals(sub) {
            let vals = possible_values(pos);
            if !vals.is_empty() {
                let val_list = vals.join(" ");
                let _ = writeln!(
                    out,
                    "complete -c {bin} -n '{cond}; and not __fish_seen_subcommand_from {val_list}' -f -a '{val_list}'"
                );
            }
        }
    }
    out
}

fn write_fish_opt(out: &mut String, bin: &str, cond: &str, arg: &Arg) {
    let desc = fish_escape(&first_line_help(arg.get_help()));
    let takes_val = arg.get_action().takes_values();
    let vals = possible_values(arg);
    let mut line = format!("complete -c {bin} -n '{cond}'");
    if let Some(short) = arg.get_short() {
        let _ = write!(line, " -s {short}");
    }
    if let Some(long) = arg.get_long() {
        let _ = write!(line, " -l {long}");
    }
    if !vals.is_empty() {
        let _ = write!(line, " -x -a '{}'", vals.join(" "));
    } else if takes_val {
        line.push_str(" -r");
    }
    if !desc.is_empty() {
        let _ = write!(line, " -d '{desc}'");
    }
    out.push_str(&line);
    out.push('\n');
}

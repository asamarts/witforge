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
        for nested in visible_subcommands(sub) {
            for arg in visible_opts(nested) {
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
    }

    let mut out = String::new();
    let _ = writeln!(out, "# Bash completion for {bin}.");
    let _ = writeln!(out, "_{bin}() {{");
    let _ = writeln!(
        out,
        "    local cur prev prev_flag cur_flag cur_val subcmd subcmd_idx subcmd2 i j word skip_next"
    );
    let _ = writeln!(out, "    COMPREPLY=()");
    let _ = writeln!(out, "    cur=\"${{COMP_WORDS[COMP_CWORD]}}\"");
    let _ = writeln!(out, "    prev=\"\"");
    let _ = writeln!(out, "    if (( COMP_CWORD > 0 )); then");
    let _ = writeln!(out, "        prev=\"${{COMP_WORDS[COMP_CWORD-1]}}\"");
    let _ = writeln!(out, "    fi");
    let _ = writeln!(out, "    subcmd=\"\"");
    let _ = writeln!(out, "    subcmd_idx=0");
    let _ = writeln!(out, "    skip_next=0");
    let _ = writeln!(out, "    for (( i=1; i<COMP_CWORD; i++ )); do");
    let _ = writeln!(out, "        word=\"${{COMP_WORDS[i]}}\"");
    let _ = writeln!(out, "        if (( skip_next )); then");
    let _ = writeln!(out, "            if [[ \"$word\" != \"=\" ]]; then");
    let _ = writeln!(out, "                skip_next=0");
    let _ = writeln!(out, "            fi");
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
    let _ = writeln!(out, "                subcmd_idx=$i");
    let _ = writeln!(out, "                break");
    let _ = writeln!(out, "                ;;");
    let _ = writeln!(out, "        esac");
    let _ = writeln!(out, "    done");

    if !flag_value_cases.is_empty() {
        let _ = writeln!(out, "    if [[ \"$cur\" == --*=* ]]; then");
        let _ = writeln!(out, "        cur_flag=\"${{cur%%=*}}\"");
        let _ = writeln!(out, "        cur_val=\"${{cur#*=}}\"");
        let _ = writeln!(out, "        case \"$cur_flag\" in");
        for (flag, vals) in &flag_value_cases {
            let _ = writeln!(out, "            {flag})");
            let _ = writeln!(
                out,
                "                COMPREPLY=( $(compgen -P \"${{cur_flag}}=\" -W \"{vals}\" -- \"$cur_val\") )"
            );
            let _ = writeln!(out, "                return 0");
            let _ = writeln!(out, "                ;;");
        }
        let _ = writeln!(out, "        esac");
        let _ = writeln!(out, "    fi");
        let _ = writeln!(out, "    prev_flag=\"$prev\"");
        let _ = writeln!(
            out,
            "    if [[ \"$prev\" == \"=\" && COMP_CWORD -gt 1 ]]; then"
        );
        let _ = writeln!(out, "        prev_flag=\"${{COMP_WORDS[COMP_CWORD-2]}}\"");
        let _ = writeln!(out, "    fi");
        let _ = writeln!(out, "    case \"$prev_flag\" in");
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
        let nested_subs = visible_subcommands(sub);
        let pos_vals: Vec<String> = visible_positionals(sub)
            .into_iter()
            .flat_map(possible_values)
            .collect();
        let _ = writeln!(out, "        {})", sub.get_name());
        if nested_subs.is_empty() {
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
        } else {
            let nested_names: Vec<&str> = nested_subs.iter().map(|n| n.get_name()).collect();
            let _ = writeln!(out, "            subcmd2=\"\"");
            let _ = writeln!(out, "            skip_next=0");
            let _ = writeln!(
                out,
                "            for (( j=subcmd_idx+1; j<COMP_CWORD; j++ )); do"
            );
            let _ = writeln!(out, "                word=\"${{COMP_WORDS[j]}}\"");
            let _ = writeln!(out, "                if (( skip_next )); then");
            let _ = writeln!(out, "                    if [[ \"$word\" != \"=\" ]]; then");
            let _ = writeln!(out, "                        skip_next=0");
            let _ = writeln!(out, "                    fi");
            let _ = writeln!(out, "                    continue");
            let _ = writeln!(out, "                fi");
            let _ = writeln!(out, "                case \"$word\" in");
            if !global_value_flags.is_empty() {
                let pattern = global_value_flags
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join("|");
                let _ = writeln!(out, "                    {pattern})");
                let _ = writeln!(out, "                        skip_next=1");
                let _ = writeln!(out, "                        ;;");
            }
            let _ = writeln!(out, "                    -*)");
            let _ = writeln!(out, "                        ;;");
            let _ = writeln!(out, "                    *)");
            let _ = writeln!(out, "                        subcmd2=\"$word\"");
            let _ = writeln!(out, "                        break");
            let _ = writeln!(out, "                        ;;");
            let _ = writeln!(out, "                esac");
            let _ = writeln!(out, "            done");
            let _ = writeln!(out, "            if [[ -z \"$subcmd2\" ]]; then");
            let _ = writeln!(out, "                if [[ \"$cur\" == -* ]]; then");
            let _ = writeln!(
                out,
                "                    COMPREPLY=( $(compgen -W \"{}\" -- \"$cur\") )",
                flags.join(" ")
            );
            let _ = writeln!(out, "                else");
            let _ = writeln!(
                out,
                "                    COMPREPLY=( $(compgen -W \"{}\" -- \"$cur\") )",
                nested_names.join(" ")
            );
            let _ = writeln!(out, "                fi");
            let _ = writeln!(out, "            else");
            let _ = writeln!(out, "                case \"$subcmd2\" in");
            for nested in &nested_subs {
                let mut nested_flags = Vec::new();
                for arg in visible_opts(nested) {
                    nested_flags.extend(arg_tokens(arg));
                }
                let nested_pos: Vec<String> = visible_positionals(nested)
                    .into_iter()
                    .flat_map(possible_values)
                    .collect();
                let _ = writeln!(out, "                    {})", nested.get_name());
                let _ = writeln!(out, "                        if [[ \"$cur\" == -* ]]; then");
                let _ = writeln!(
                    out,
                    "                            COMPREPLY=( $(compgen -W \"{}\" -- \"$cur\") )",
                    nested_flags.join(" ")
                );
                if !nested_pos.is_empty() {
                    let _ = writeln!(out, "                        else");
                    let _ = writeln!(
                        out,
                        "                            COMPREPLY=( $(compgen -W \"{}\" -- \"$cur\") )",
                        nested_pos.join(" ")
                    );
                }
                let _ = writeln!(out, "                        fi");
                let _ = writeln!(out, "                        ;;");
            }
            let _ = writeln!(out, "                esac");
            let _ = writeln!(out, "            fi");
        }
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
    let optional_val = takes_val
        && arg
            .get_num_args()
            .is_some_and(|range| range.min_values() == 0);
    let vals = possible_values(arg);
    let value_suffix = if takes_val {
        let value_name = arg
            .get_value_names()
            .and_then(|names| names.first())
            .map_or("VALUE", clap::builder::Str::as_str);
        let escaped_name = zsh_escape(value_name);
        let sep = if optional_val { "::" } else { ":" };
        if !vals.is_empty() {
            format!("{sep}{escaped_name}:({})", vals.join(" "))
        } else if matches!(value_name, "DIR" | "WIT_DIR" | "WIT_ROOT" | "OLD" | "NEW") {
            format!("{sep}{escaped_name}:_files -/")
        } else if matches!(value_name, "PATH") {
            format!("{sep}{escaped_name}:_files")
        } else {
            format!("{sep}{escaped_name}: ")
        }
    } else {
        String::new()
    };

    let eq = if optional_val && arg.is_require_equals_set() {
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
        let nested_subs = visible_subcommands(sub);
        if nested_subs.is_empty() {
            write_zsh_leaf_fn(&mut out, &format!("_{bin}_{fn_suffix}"), sub);
        } else {
            let _ = writeln!(out, "_{bin}_{fn_suffix}() {{");
            let _ = writeln!(out, "    local context state state_descr line");
            let _ = writeln!(out, "    typeset -A opt_args");
            let _ = writeln!(out, "    _arguments -C \\");
            for arg in visible_opts(sub) {
                for spec in zsh_arg_spec(arg) {
                    let _ = writeln!(out, "        {spec} \\");
                }
            }
            let _ = writeln!(out, "        '1: :_{bin}_{fn_suffix}_commands' \\");
            let _ = writeln!(out, "        '*::arg:->args'");
            let _ = writeln!(out, "    case $state in");
            let _ = writeln!(out, "        args)");
            let _ = writeln!(out, "            case $line[1] in");
            for nested in &nested_subs {
                let nested_suffix = nested.get_name().replace('-', "_");
                let _ = writeln!(
                    out,
                    "                {}) _{bin}_{fn_suffix}_{nested_suffix} ;;",
                    nested.get_name()
                );
            }
            let _ = writeln!(out, "            esac");
            let _ = writeln!(out, "            ;;");
            let _ = writeln!(out, "    esac");
            let _ = writeln!(out, "}}\n");

            let _ = writeln!(out, "_{bin}_{fn_suffix}_commands() {{");
            let _ = writeln!(out, "    local commands; commands=(");
            for nested in &nested_subs {
                let desc = zsh_escape(&first_line_help(nested.get_about()));
                let _ = writeln!(out, "        '{}:{desc}'", nested.get_name());
            }
            let _ = writeln!(out, "    )");
            let _ = writeln!(
                out,
                "    _describe -t commands '{bin} {} commands' commands",
                sub.get_name()
            );
            let _ = writeln!(out, "}}\n");

            for nested in &nested_subs {
                let nested_suffix = nested.get_name().replace('-', "_");
                write_zsh_leaf_fn(
                    &mut out,
                    &format!("_{bin}_{fn_suffix}_{nested_suffix}"),
                    nested,
                );
            }
        }
    }

    let _ = writeln!(
        out,
        "if [ \"$funcstack[1]\" = \"_{bin}\" ]; then\n    _{bin} \"$@\"\nelse\n    compdef _{bin} {bin}\nfi"
    );
    out
}

fn write_zsh_leaf_fn(out: &mut String, fn_name: &str, cmd: &Command) {
    let _ = writeln!(out, "{fn_name}() {{");
    let mut specs = Vec::new();
    for arg in visible_opts(cmd) {
        specs.extend(zsh_arg_spec(arg));
    }
    for (idx, pos) in visible_positionals(cmd).into_iter().enumerate() {
        let ordinal = idx + 1;
        let name = pos
            .get_value_names()
            .and_then(|names| names.first())
            .map_or("ARG", clap::builder::Str::as_str);
        let escaped_name = zsh_escape(name);
        let vals = possible_values(pos);
        if !vals.is_empty() {
            specs.push(format!("'{ordinal}:{escaped_name}:({})'", vals.join(" ")));
        } else if name == "DIR" {
            specs.push(format!("'{ordinal}:{escaped_name}:_files -/'"));
        } else if matches!(name, "PATH" | "WIT_DIR" | "WIT_ROOT" | "OLD" | "NEW") {
            specs.push(format!("'{ordinal}:{escaped_name}:_files'"));
        } else {
            specs.push(format!("'{ordinal}:{escaped_name}: '"));
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
        let nested_subs = visible_subcommands(sub);
        if !nested_subs.is_empty() {
            let other_nested_names = nested_subs
                .iter()
                .map(|nested| nested.get_name())
                .filter(|nested_name| *nested_name != name)
                .collect::<Vec<_>>()
                .join(" ");
            let parent_cond = if other_nested_names.is_empty() {
                format!("__fish_seen_subcommand_from {name}")
            } else {
                format!(
                    "__fish_seen_subcommand_from {name}; and not __fish_seen_subcommand_from \
                     {other_nested_names}"
                )
            };
            for arg in visible_opts(sub) {
                write_fish_opt(&mut out, bin, &parent_cond, arg);
            }
            for nested in &nested_subs {
                let nested_name = nested.get_name();
                let desc = fish_escape(&first_line_help(nested.get_about()));
                let _ = writeln!(
                    out,
                    "complete -c {bin} -n '{parent_cond}' -f -a '{nested_name}' -d '{desc}'"
                );
            }
            for nested in &nested_subs {
                let nested_name = nested.get_name();
                let nested_cond = format!(
                    "__fish_seen_subcommand_from {name}; and __fish_seen_subcommand_from \
                     {nested_name}"
                );
                for arg in visible_opts(nested) {
                    write_fish_opt(&mut out, bin, &nested_cond, arg);
                }
                for pos in visible_positionals(nested) {
                    let vals = possible_values(pos);
                    if !vals.is_empty() {
                        let val_list = vals.join(" ");
                        let _ = writeln!(
                            out,
                            "complete -c {bin} -n '{nested_cond}; and not \
                             __fish_seen_subcommand_from {val_list}' -f -a '{val_list}'"
                        );
                    }
                }
            }
            continue;
        }
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
    let optional_val = arg.is_require_equals_set()
        || arg
            .get_num_args()
            .is_some_and(|range| range.min_values() == 0);
    if !vals.is_empty() {
        let _ = write!(line, " -x -a '{}'", vals.join(" "));
    } else if takes_val && !optional_val {
        line.push_str(" -r");
    }
    if !desc.is_empty() {
        let _ = write!(line, " -d '{desc}'");
    }
    out.push_str(&line);
    out.push('\n');
}

//! Normalisation and escaping of WIT documentation comments for Rust and protobuf emitters.
//!
//! WIT doc comments are kept as readable Markdown rather than wrapped in a whole-comment code
//! fence:
//!
//! - All line endings and Unicode line/paragraph separators (`\r\n`, bare `\r`, vertical tab,
//!   form feed, `U+0085`, `U+2028`, `U+2029`) are normalised to LF, and NUL and other C0/C1
//!   control characters (except `\n` and `\t`) are stripped before emission.
//! - Decorative leading `*` markers left by WIT `/** ... */` block comments are stripped.
//! - Every code block (fenced with backticks or tildes, labelled or unlabelled, or indented by
//!   four or more columns) is emitted as a closed fenced code block marked `text`, so rustdoc
//!   never compiles it as a doctest.
//! - Outside fenced code blocks and balanced inline code spans, `<` and `>` are escaped (keeping
//!   `<http(s)://...>` autolinks), non-external `[...]` links are escaped while real inline
//!   `[text](url)` links keep working, bare `http(s)://` URLs are wrapped in `<...>` for Rust
//!   (and kept bare for protobuf so `prost-build`'s URL wrapper does not double-wrap them),
//!   unbalanced backticks are escaped, and a lone trailing `\` is escaped.
//! - Non-empty emitted lines always carry a single space after `///`, `//!`, or `//`, so lines
//!   beginning with `/` or `!` cannot turn a doc comment into another comment kind.
//! - Long lines are never re-wrapped.

use std::fmt::Write as _;

use anyhow::Result;

/// Target syntax for Markdown escaping.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Target {
    /// Rustdoc `///` or `//!` comments.
    Rust,
    /// Protobuf `//` comments (also consumed by `prost-build` to emit rustdoc).
    Proto,
}

/// Write outer Rust doc comments (`///`) for `docs`, indented by `indent` spaces. Returns `true`
/// if any line was written.
pub(crate) fn write_rust_outer(out: &mut String, indent: usize, docs: &str) -> Result<bool> {
    let lines = sanitize_rust_lines(docs);
    if lines.is_empty() {
        return Ok(false);
    }
    let prefix = " ".repeat(indent);
    for line in lines {
        if line.is_empty() {
            writeln!(out, "{prefix}///")?;
        } else {
            writeln!(out, "{prefix}/// {line}")?;
        }
    }
    Ok(true)
}

/// Write inner Rust doc comments (`//!`) for `docs`. Returns `true` if any line was written.
pub(crate) fn write_rust_inner(out: &mut String, docs: &str) -> Result<bool> {
    let lines = sanitize_rust_lines(docs);
    if lines.is_empty() {
        return Ok(false);
    }
    for line in lines {
        if line.is_empty() {
            writeln!(out, "//!")?;
        } else {
            writeln!(out, "//! {line}")?;
        }
    }
    Ok(true)
}

/// Write protobuf line comments (`//`) for `docs`, indented by `indent` spaces.
pub(crate) fn write_proto(out: &mut String, indent: usize, docs: &str) -> Result<()> {
    let lines = sanitize_proto_lines(docs);
    if lines.is_empty() {
        return Ok(());
    }
    let prefix = " ".repeat(indent);
    for line in lines {
        if line.is_empty() {
            writeln!(out, "{prefix}//")?;
        } else {
            writeln!(out, "{prefix}// {line}")?;
        }
    }
    Ok(())
}

/// Normalise and escape `docs` into lines safe for Rust `///` / `//!` doc comments.
pub(crate) fn sanitize_rust_lines(docs: &str) -> Vec<String> {
    let lines = normalize_lines(docs);
    if lines.is_empty() {
        return Vec::new();
    }
    sanitize_markdown_lines(&lines, Target::Rust)
}

/// Normalise and escape `docs` into lines safe for protobuf `//` comments (including `prost-build`).
pub(crate) fn sanitize_proto_lines(docs: &str) -> Vec<String> {
    let lines = normalize_lines(docs);
    if lines.is_empty() {
        return Vec::new();
    }
    sanitize_markdown_lines(&lines, Target::Proto)
}

/// Normalise WIT documentation into plain LF-separated text for JSON Schema descriptions and the
/// contract IR.
///
/// Normalises line endings and Unicode line/paragraph separators to `\n`, strips C0/C1 control
/// characters (except `\n` and `\t`), strips decorative `/** *` block-comment gutters, and trims
/// leading and trailing blank lines.
pub(crate) fn normalize_plain(docs: &str) -> String {
    normalize_lines(docs).join("\n")
}

fn normalize_lines(raw: &str) -> Vec<String> {
    if raw.is_empty() {
        return Vec::new();
    }
    let mut normalized = String::with_capacity(raw.len());
    let mut chars = raw.chars().peekable();
    while let Some(ch) = chars.next() {
        match ch {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                normalized.push('\n');
            }
            '\n' | '\u{000B}' | '\u{000C}' | '\u{0085}' | '\u{2028}' | '\u{2029}' => {
                normalized.push('\n');
            }
            '\t' => normalized.push('\t'),
            ch if ch.is_control() => {}
            ch => normalized.push(ch),
        }
    }
    let mut lines: Vec<String> = normalized
        .split('\n')
        .map(|line| line.trim_end().to_string())
        .collect();
    trim_blank_edges(&mut lines);
    if lines.is_empty() {
        return lines;
    }
    strip_block_comment_asterisks(&mut lines);
    trim_blank_edges(&mut lines);
    lines
}

fn trim_blank_edges(lines: &mut Vec<String>) {
    while lines.first().is_some_and(String::is_empty) {
        lines.remove(0);
    }
    while lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
}

/// Strip decorative leading `*` markers left over from WIT `/** ... */` block comments.
///
/// `wit-parser` strips the outer `/**` and `*/` delimiters but preserves leading indentation and
/// decorative `*` prefixes on inner lines of block comments. This strips them only when the lines
/// match a block-comment gutter so normal `/// * bullet` Markdown lists are preserved.
#[allow(clippy::too_many_lines)]
fn strip_block_comment_asterisks(lines: &mut [String]) {
    fn parse_star(line: &str) -> Option<(&str, &str)> {
        let ws_len = line
            .bytes()
            .take_while(|&b| b == b' ' || b == b'\t')
            .count();
        let gutter = &line[..ws_len];
        let rest = &line[ws_len..];
        let after = rest.strip_prefix('*')?;
        if after.is_empty() {
            Some((gutter, ""))
        } else if let Some(body) = after.strip_prefix(' ') {
            Some((gutter, body))
        } else if after.starts_with('\t') {
            Some((gutter, after))
        } else {
            None
        }
    }

    fn is_closing_star_banner(line: &str, expected_gutter: &str) -> bool {
        let ws_len = line
            .bytes()
            .take_while(|&b| b == b' ' || b == b'\t')
            .count();
        let ws = &line[..ws_len];
        let rest = &line[ws_len..];
        if rest.is_empty() || !rest.bytes().all(|b| b == b'*') {
            return false;
        }
        // Same gutter as body lines (` * ` / ` **`), aligned 1 column left of the body gutter
        // (`/***\n * text\n***/` or `  /***\n   * text\n  ***/`), or column 0 when body is
        // indented.
        (ws == expected_gutter && rest.len() >= 2)
            || expected_gutter
                .strip_suffix(' ')
                .is_some_and(|aligned| ws == aligned)
            || (ws.is_empty() && !expected_gutter.is_empty())
    }

    fn is_standalone_closing_star_banner(line: &str) -> bool {
        let ws_len = line
            .bytes()
            .take_while(|&b| b == b' ' || b == b'\t')
            .count();
        let rest = &line[ws_len..];
        rest.len() >= 2 && rest.bytes().all(|b| b == b'*')
    }

    fn strip_inline_closing_stars(line: &str) -> Option<String> {
        let trimmed_start = line.trim_start_matches([' ', '\t']);
        // Pure `**+` banner line (`/** ***/`, `/*****/`).
        if trimmed_start.len() >= 2 && trimmed_start.bytes().all(|b| b == b'*') {
            return Some(String::new());
        }
        // Symmetric `/*** Hello. ***/`, `/**** Hello. ****/` (or `/*** ***/`).
        let after_lead = trimmed_start.trim_start_matches('*');
        if after_lead.len() < trimmed_start.len()
            && (after_lead.is_empty() || after_lead.starts_with([' ', '\t']))
        {
            let no_stars = after_lead.trim_end_matches('*');
            if no_stars.len() < after_lead.len()
                && (no_stars.is_empty() || no_stars.ends_with([' ', '\t']))
            {
                return Some(no_stars.trim().to_string());
            }
        }
        // Single-line `/** Hello. ***/` leaves trailing `<ws>**+` (>= 2 stars). Require >= 2
        // trailing stars so `/// Pointer T *` and `/// Match pattern *` are never corrupted.
        let no_stars = line.trim_end_matches('*');
        if line.len() - no_stars.len() >= 2 && no_stars.ends_with([' ', '\t']) {
            return Some(no_stars.trim().to_string());
        }
        None
    }

    // Single-line block comment banners (`/*** Hello. ***/`, `/** Hello. ***/`, `/*****/`).
    if lines.len() == 1
        && let Some(stripped) = strip_inline_closing_stars(&lines[0])
    {
        lines[0] = stripped;
        return;
    }

    // All-banner multi-line block comments (`/****\n****/`, `/****\n ****/`).
    if lines.len() >= 2
        && lines.iter().any(|l| {
            let t = l.trim_start_matches([' ', '\t']);
            t.len() >= 2 && t.bytes().all(|b| b == b'*')
        })
        && lines.iter().all(|l| {
            let t = l.trim_start_matches([' ', '\t']);
            t.is_empty() || t.bytes().all(|b| b == b'*')
        })
    {
        for line in lines.iter_mut() {
            line.clear();
        }
        return;
    }

    // Form 1: every non-empty line shares the same `<spaces/tabs>*` gutter (non-empty gutter or
    // at least one bare `*` blank line), optionally preceded by a `*`-only opening banner line
    // (`/***`) and/or ending in an aligned `*`-only closing banner line (`***/`, `**/`).
    let has_opening_banner =
        lines.len() >= 2 && !lines[0].is_empty() && lines[0].bytes().all(|b| b == b'*');
    let start_idx = usize::from(has_opening_banner);
    let mut all_match = true;
    let mut gutter: Option<&str> = None;
    let mut has_bare_star = false;
    let mut has_closing_banner = false;
    let last_idx = lines.len().saturating_sub(1);
    for (idx, line) in lines.iter().enumerate().skip(start_idx) {
        if line.is_empty() {
            continue;
        }
        if idx == last_idx
            && lines.len() > start_idx + 1
            && let Some(g) = gutter
            && is_closing_star_banner(line, g)
        {
            has_closing_banner = true;
            continue;
        }
        let Some((ws, body)) = parse_star(line) else {
            all_match = false;
            break;
        };
        if body.is_empty() {
            has_bare_star = true;
        }
        match gutter {
            None => gutter = Some(ws),
            Some(prev) if prev != ws => {
                all_match = false;
                break;
            }
            Some(_) => {}
        }
    }
    // For a zero-space `*` gutter (`ws.is_empty()`), a single `***`+ line at either edge is a
    // valid CommonMark thematic break adjacent to a `/// * bullet` list, so strip only when
    // disambiguated by a non-empty gutter, bare `*` blank line, both banners, or `< 3` stars.
    let strip_form1 = all_match
        && gutter.is_some_and(|ws| {
            !ws.is_empty()
                || has_bare_star
                || (has_opening_banner && has_closing_banner)
                || (has_opening_banner && lines[0].len() < 3)
                || (has_closing_banner && lines[last_idx].trim().len() < 3)
        });
    if strip_form1 {
        if has_opening_banner {
            lines[0].clear();
        }
        for (idx, line) in lines.iter_mut().enumerate().skip(start_idx) {
            if idx == last_idx && has_closing_banner {
                line.clear();
            } else if let Some((_, body)) = parse_star(line) {
                *line = body.to_string();
            }
        }
        return;
    }

    // Form 2: `/** First line\n * Second line\n */` (and 2-line `/** First line.\n ***/`) where
    // line 0 is prose and every non-empty continuation line shares a `<spaces/tabs>*` gutter
    // (optionally followed by an aligned closing `*`-only banner line). Preserve 0-space (`*`),
    // 2-space (`  *`), and colon-introduced 1-space (`Options:\n * a`) `///` Markdown bullet lists,
    // as well as `/// ***` thematic breaks after `///` prose or 0-space bullet lists.
    if lines.len() >= 2 && list_marker_content_col(&lines[0]).is_none() {
        let mut tail_match = true;
        let mut tail_gutter: Option<&str> = None;
        let mut tail_bare_star = false;
        let mut tail_closing_banner = false;
        for (idx, line) in lines.iter().enumerate().skip(1) {
            if line.is_empty() {
                continue;
            }
            if idx == last_idx
                && match tail_gutter {
                    Some(g) => is_closing_star_banner(line, g),
                    None => is_standalone_closing_star_banner(line),
                }
            {
                tail_closing_banner = true;
                continue;
            }
            let Some((ws, body)) = parse_star(line) else {
                tail_match = false;
                break;
            };
            if body.is_empty() {
                tail_bare_star = true;
            }
            match tail_gutter {
                None => tail_gutter = Some(ws),
                Some(prev) if prev != ws => {
                    tail_match = false;
                    break;
                }
                Some(_) => {}
            }
        }
        let closing_disambiguated = tail_closing_banner
            && (lines[0].starts_with([' ', '\t']) || lines[last_idx].trim().len() < 3);
        let strip_form2 = tail_match
            && match tail_gutter {
                Some(ws) => {
                    (lines[0].starts_with([' ', '\t']) && !ws.is_empty())
                        || ws.contains('\t')
                        || ws.len() >= 3
                        || (ws == " " && !lines[0].trim_end().ends_with(':'))
                        || tail_bare_star
                        || (!ws.is_empty() && tail_closing_banner)
                        || closing_disambiguated
                }
                None => tail_closing_banner && (lines.len() == 2 || closing_disambiguated),
            };
        if strip_form2 {
            lines[0] = lines[0].trim_start_matches([' ', '\t']).to_string();
            for (idx, line) in lines.iter_mut().enumerate().skip(1) {
                if idx == last_idx && tail_closing_banner {
                    line.clear();
                } else if let Some((_, body)) = parse_star(line) {
                    *line = body.to_string();
                }
            }
        }
    }
}

#[allow(clippy::too_many_lines)]
fn sanitize_markdown_lines(lines: &[String], target: Target) -> Vec<String> {
    let mut out = Vec::with_capacity(lines.len());
    let mut pending_paragraph: Vec<String> = Vec::new();
    let mut in_paragraph = false;
    let mut list_cols: Vec<usize> = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        let line = &lines[i];
        if line.is_empty() {
            flush_paragraph(&mut pending_paragraph, &mut out, target);
            in_paragraph = false;
            out.push(String::new());
            i += 1;
            continue;
        }

        if is_blockquote(line) {
            flush_paragraph(&mut pending_paragraph, &mut out, target);
            in_paragraph = false;
            list_cols.clear();
            let mut inner = Vec::new();
            while i < lines.len() && is_blockquote(&lines[i]) {
                inner.push(strip_blockquote(&lines[i]));
                i += 1;
            }
            for sanitized in sanitize_markdown_lines(&inner, target) {
                if sanitized.is_empty() {
                    out.push(">".to_string());
                } else {
                    out.push(format!("> {sanitized}"));
                }
            }
            continue;
        }

        let (_, col) = leading_columns(line);
        // After a blank line (not continuing a paragraph), any non-blank line indented less than
        // an enclosing list item's content column terminates that list level in CommonMark.
        if !in_paragraph {
            while list_cols.last().is_some_and(|&lc| col < lc) {
                list_cols.pop();
            }
        }
        let current_list_col = list_cols.last().copied();
        let indented_code_threshold = current_list_col.map_or(4, |c| c + 4);
        let max_fence_indent = current_list_col.map_or(3, |c| c + 3);
        let is_indented_code = !in_paragraph && col >= indented_code_threshold;

        if !is_indented_code
            && let Some((prefix, fence_char, fence_len)) = detect_fence(line, max_fence_indent)
        {
            flush_paragraph(&mut pending_paragraph, &mut out, target);
            in_paragraph = false;
            // In CommonMark §4.5, a fenced code block interrupting a paragraph at `col < lc`
            // terminates the enclosing list level(s).
            while list_cols.last().is_some_and(|&lc| col < lc) {
                list_cols.pop();
            }
            let close_max_indent = list_cols.last().copied().map_or(3, |c| c + 3);
            let expanded_prefix = expand_leading_ws(prefix);
            let fence: String = std::iter::repeat_n(fence_char, fence_len).collect();
            out.push(format!("{expanded_prefix}{fence}text"));
            i += 1;
            let mut closed = false;
            while i < lines.len() {
                if is_closing_fence(&lines[i], fence_char, fence_len, close_max_indent) {
                    out.push(format!("{expanded_prefix}{fence}"));
                    i += 1;
                    closed = true;
                    break;
                }
                out.push(lines[i].clone());
                i += 1;
            }
            if !closed {
                out.push(format!("{expanded_prefix}{fence}"));
            }
            continue;
        }

        if is_indented_code {
            flush_paragraph(&mut pending_paragraph, &mut out, target);
            let list_indent = current_list_col.unwrap_or(0);
            let indent_prefix = " ".repeat(list_indent);
            let start = i;
            while i < lines.len() {
                if lines[i].is_empty() || leading_columns(&lines[i]).1 >= indented_code_threshold {
                    i += 1;
                } else {
                    break;
                }
            }
            // Leave trailing blank lines outside the code fence.
            let mut end = i;
            while end > start && lines[end - 1].is_empty() {
                end -= 1;
            }
            i = end;
            let dedented: Vec<String> = lines[start..end]
                .iter()
                .map(|code_line| {
                    if code_line.is_empty() {
                        String::new()
                    } else {
                        strip_columns(code_line, indented_code_threshold)
                    }
                })
                .collect();
            let fence_len = max_backtick_run(&dedented).max(2) + 1;
            let fence = "`".repeat(fence_len);
            out.push(format!("{indent_prefix}{fence}text"));
            for code_line in dedented {
                if code_line.is_empty() {
                    out.push(String::new());
                } else {
                    out.push(format!("{indent_prefix}{code_line}"));
                }
            }
            out.push(format!("{indent_prefix}{fence}"));
            continue;
        }

        if is_atx_heading_or_thematic_break(line) {
            flush_paragraph(&mut pending_paragraph, &mut out, target);
            in_paragraph = false;
            while list_cols.last().is_some_and(|&lc| col < lc) {
                list_cols.pop();
            }
            out.extend(escape_paragraph_lines(std::slice::from_ref(line), target));
            i += 1;
            continue;
        }

        if let Some(marker) = parse_list_marker(line, max_fence_indent) {
            flush_paragraph(&mut pending_paragraph, &mut out, target);
            while list_cols.last().is_some_and(|&lc| col < lc) {
                list_cols.pop();
            }
            if list_cols.last().copied() != Some(marker.content_col) {
                list_cols.push(marker.content_col);
            }
            if let Some((marker_prefix, first_code)) = marker.first_line_code {
                in_paragraph = false;
                out.push(marker_prefix);
                let code_threshold = marker.content_col + 4;
                let indent_prefix = " ".repeat(marker.content_col);
                let mut dedented = vec![first_code];
                i += 1;
                let cont_start = i;
                while i < lines.len() {
                    if lines[i].is_empty() || leading_columns(&lines[i]).1 >= code_threshold {
                        i += 1;
                    } else {
                        break;
                    }
                }
                let mut cont_end = i;
                while cont_end > cont_start && lines[cont_end - 1].is_empty() {
                    cont_end -= 1;
                }
                i = cont_end;
                for code_line in &lines[cont_start..cont_end] {
                    if code_line.is_empty() {
                        dedented.push(String::new());
                    } else {
                        dedented.push(strip_columns(code_line, code_threshold));
                    }
                }
                let fence_len = max_backtick_run(&dedented).max(2) + 1;
                let fence = "`".repeat(fence_len);
                out.push(format!("{indent_prefix}{fence}text"));
                for code_line in dedented {
                    if code_line.is_empty() {
                        out.push(String::new());
                    } else {
                        out.push(format!("{indent_prefix}{code_line}"));
                    }
                }
                out.push(format!("{indent_prefix}{fence}"));
                continue;
            }
            in_paragraph = true;
            pending_paragraph.push(line.clone());
            i += 1;
            continue;
        }

        in_paragraph = true;
        pending_paragraph.push(line.clone());
        i += 1;
    }

    flush_paragraph(&mut pending_paragraph, &mut out, target);
    out
}

fn flush_paragraph(pending: &mut Vec<String>, out: &mut Vec<String>, target: Target) {
    if pending.is_empty() {
        return;
    }
    out.extend(escape_paragraph_lines(pending, target));
    pending.clear();
}

fn leading_columns(line: &str) -> (usize, usize) {
    let mut byte_off = 0;
    let mut col = 0;
    for (idx, ch) in line.char_indices() {
        match ch {
            ' ' => {
                col += 1;
                byte_off = idx + 1;
            }
            '\t' => {
                col += 4 - (col % 4);
                byte_off = idx + 1;
            }
            _ => break,
        }
    }
    (byte_off, col)
}

fn expand_leading_ws(ws: &str) -> String {
    let mut out = String::new();
    let mut col = 0;
    for ch in ws.chars() {
        match ch {
            ' ' => {
                out.push(' ');
                col += 1;
            }
            '\t' => {
                let width = 4 - (col % 4);
                for _ in 0..width {
                    out.push(' ');
                }
                col += width;
            }
            _ => out.push(ch),
        }
    }
    out
}

fn strip_columns(line: &str, cols_to_strip: usize) -> String {
    let mut col = 0;
    for (idx, ch) in line.char_indices() {
        if col >= cols_to_strip {
            return line[idx..].to_string();
        }
        match ch {
            ' ' => col += 1,
            '\t' => {
                let next_col = col + (4 - (col % 4));
                if next_col > cols_to_strip {
                    let mut out = " ".repeat(next_col - cols_to_strip);
                    out.push_str(&line[idx + 1..]);
                    return out;
                }
                col = next_col;
            }
            _ => return line[idx..].to_string(),
        }
    }
    String::new()
}

fn detect_fence(line: &str, max_indent: usize) -> Option<(&str, char, usize)> {
    let (byte_off, col) = leading_columns(line);
    if col > max_indent {
        return None;
    }
    let rest = &line[byte_off..];
    let fence_char = rest.chars().next()?;
    if !matches!(fence_char, '`' | '~') {
        return None;
    }
    let fence_len = rest.bytes().take_while(|&b| b == fence_char as u8).count();
    if fence_len < 3 {
        return None;
    }
    let info = &rest[fence_len..];
    if fence_char == '`' && info.contains('`') {
        return None;
    }
    Some((&line[..byte_off], fence_char, fence_len))
}

fn is_closing_fence(line: &str, fence_char: char, min_len: usize, max_indent: usize) -> bool {
    let (byte_off, col) = leading_columns(line);
    if col > max_indent {
        return false;
    }
    let rest = &line[byte_off..];
    let len = rest.bytes().take_while(|&b| b == fence_char as u8).count();
    len >= min_len && len == rest.len()
}

fn max_backtick_run(lines: &[String]) -> usize {
    let mut max_run = 0;
    for line in lines {
        let mut current = 0;
        for b in line.bytes() {
            if b == b'`' {
                current += 1;
                max_run = max_run.max(current);
            } else {
                current = 0;
            }
        }
    }
    max_run
}

fn is_blockquote(line: &str) -> bool {
    let (byte_off, col) = leading_columns(line);
    col <= 3 && line[byte_off..].starts_with('>')
}

fn strip_blockquote(line: &str) -> String {
    let (byte_off, _) = leading_columns(line);
    let after = &line[byte_off + 1..];
    if let Some(rest) = after.strip_prefix(' ') {
        rest.to_string()
    } else if let Some(rest) = after.strip_prefix('\t') {
        format!("   {rest}")
    } else {
        after.to_string()
    }
}

fn is_atx_heading_or_thematic_break(line: &str) -> bool {
    let (byte_off, col) = leading_columns(line);
    if col > 3 {
        return false;
    }
    let rest = &line[byte_off..];
    let hashes = rest.bytes().take_while(|&b| b == b'#').count();
    if (1..=6).contains(&hashes) {
        let after = &rest[hashes..];
        if after.is_empty() || after.starts_with(' ') || after.starts_with('\t') {
            return true;
        }
    }
    let Some(first) = rest.chars().next() else {
        return false;
    };
    if matches!(first, '-' | '_' | '*') {
        let mut count = 0;
        if rest.chars().all(|ch| {
            if ch == first {
                count += 1;
                true
            } else {
                matches!(ch, ' ' | '\t')
            }
        }) && count >= 3
        {
            return true;
        }
    }
    false
}

struct ListMarker {
    content_col: usize,
    first_line_code: Option<(String, String)>,
}

fn parse_list_marker(line: &str, max_marker_indent: usize) -> Option<ListMarker> {
    let (byte_off, col) = leading_columns(line);
    if col > max_marker_indent {
        return None;
    }
    let rest = &line[byte_off..];
    let bytes = rest.as_bytes();
    let marker_len = if let Some(&first) = bytes.first()
        && matches!(first, b'-' | b'+' | b'*')
    {
        1
    } else {
        let digits = bytes.iter().take_while(|b| b.is_ascii_digit()).count();
        if (1..=9).contains(&digits) && matches!(bytes.get(digits), Some(b'.' | b')')) {
            digits + 1
        } else {
            return None;
        }
    };
    let marker_end_col = col + marker_len;
    let after_marker = &rest[marker_len..];
    if after_marker.is_empty() {
        return Some(ListMarker {
            content_col: marker_end_col + 1,
            first_line_code: None,
        });
    }
    if !matches!(after_marker.as_bytes()[0], b' ' | b'\t') {
        return None;
    }
    let mut ws_bytes = 0usize;
    let mut ws_col = marker_end_col;
    for b in after_marker.bytes() {
        match b {
            b' ' => {
                ws_bytes += 1;
                ws_col += 1;
            }
            b'\t' => {
                ws_bytes += 1;
                ws_col += 4 - (ws_col % 4);
            }
            _ => break,
        }
    }
    let body = &after_marker[ws_bytes..];
    if body.is_empty() {
        return Some(ListMarker {
            content_col: marker_end_col + 1,
            first_line_code: None,
        });
    }
    let ws_width = ws_col - marker_end_col;
    if ws_width >= 5 {
        // CommonMark §5.2: >= 5 columns of whitespace after a list marker begins an indented code
        // block on the first line (content_col = marker_end_col + 1, code indent = content_col + 4).
        let marker_prefix = format!(
            "{}{}",
            expand_leading_ws(&line[..byte_off]),
            &rest[..marker_len]
        );
        let extra_spaces = " ".repeat(ws_width - 5);
        return Some(ListMarker {
            content_col: marker_end_col + 1,
            first_line_code: Some((marker_prefix, format!("{extra_spaces}{body}"))),
        });
    }
    Some(ListMarker {
        content_col: ws_col,
        first_line_code: None,
    })
}

fn list_marker_content_col(line: &str) -> Option<usize> {
    parse_list_marker(line, 3).map(|marker| marker.content_col)
}

fn escape_paragraph_lines(lines: &[String], target: Target) -> Vec<String> {
    let joined = lines.join("\n");
    let escaped = escape_inline(&joined, target, true);
    escaped.split('\n').map(str::to_string).collect()
}

/// Escape inline Markdown inside a single paragraph (which may contain `\n` line breaks).
///
/// When `allow_links` is `true`, real inline `[label](url)` links and bare URLs are handled;
/// when `false` (inside an existing link's label), brackets and angle brackets are escaped and
/// bare URLs are left unwrapped.
#[allow(clippy::too_many_lines)]
fn escape_inline(text: &str, target: Target, allow_links: bool) -> String {
    let mut out = String::with_capacity(text.len());
    let bytes = text.as_bytes();
    let mut pos = 0;

    while pos < bytes.len() {
        match bytes[pos] {
            b'\\' => {
                if pos + 1 == bytes.len() || bytes[pos + 1] == b'\n' {
                    out.push_str("\\\\");
                    pos += 1;
                } else if bytes[pos + 1].is_ascii_punctuation() {
                    out.push_str(&text[pos..pos + 2]);
                    pos += 2;
                } else if allow_links && scan_bare_url(&text[pos + 1..]).is_some() {
                    // `\<https://...>` escapes `<` in CommonMark, turning the autolink back into
                    // a bare URL that fails `rustdoc::bare_urls`. Double the backslash in Rust
                    // docs so CommonMark renders a literal `\` followed by the `<url>` autolink.
                    match target {
                        Target::Rust => out.push_str("\\\\"),
                        Target::Proto => out.push('\\'),
                    }
                    pos += 1;
                } else {
                    out.push('\\');
                    pos += 1;
                }
            }
            b'`' => {
                let run_len = bytes[pos..].iter().take_while(|&&b| b == b'`').count();
                if let Some(close_end) = find_closing_backticks(bytes, pos + run_len, run_len) {
                    out.push_str(&text[pos..close_end]);
                    pos = close_end;
                } else {
                    for _ in 0..run_len {
                        out.push_str("\\`");
                    }
                    pos += run_len;
                }
            }
            b'<' => {
                if allow_links && let Some((url, end)) = parse_url_autolink(&text[pos..]) {
                    match target {
                        Target::Rust => {
                            out.push('<');
                            out.push_str(url);
                            out.push('>');
                        }
                        Target::Proto => out.push_str(url),
                    }
                    pos += end;
                } else {
                    out.push_str("\\<");
                    pos += 1;
                }
            }
            b'>' => {
                out.push_str("\\>");
                pos += 1;
            }
            b'[' => {
                if allow_links
                    && let Some((label, dest, consumed)) = parse_external_link(&text[pos..])
                {
                    out.push('[');
                    out.push_str(&escape_inline(label, target, false));
                    out.push_str("](");
                    out.push_str(&render_link_dest(dest, target));
                    out.push(')');
                    pos += consumed;
                } else {
                    out.push_str("\\[");
                    pos += 1;
                }
            }
            b']' => {
                out.push_str("\\]");
                pos += 1;
            }
            b'h' if allow_links && is_url_boundary(text, pos) => {
                if let Some(url_len) = scan_bare_url(&text[pos..]) {
                    let url = &text[pos..pos + url_len];
                    match target {
                        Target::Rust => {
                            out.push('<');
                            out.push_str(url);
                            out.push('>');
                        }
                        Target::Proto => out.push_str(url),
                    }
                    pos += url_len;
                } else {
                    out.push('h');
                    pos += 1;
                }
            }
            _ => {
                let ch = text[pos..].chars().next().unwrap();
                out.push(ch);
                pos += ch.len_utf8();
            }
        }
    }
    out
}

fn find_closing_backticks(bytes: &[u8], mut pos: usize, run_len: usize) -> Option<usize> {
    while pos < bytes.len() {
        if bytes[pos] == b'`' {
            let count = bytes[pos..].iter().take_while(|&&b| b == b'`').count();
            if count == run_len {
                return Some(pos + count);
            }
            pos += count;
        } else {
            pos += 1;
        }
    }
    None
}

fn parse_url_autolink(slice: &str) -> Option<(&str, usize)> {
    if !(slice.starts_with("<http://") || slice.starts_with("<https://")) {
        return None;
    }
    let close = slice[1..].find('>')? + 1;
    let url = &slice[1..close];
    if url.contains(|ch: char| ch.is_whitespace() || ch == '<') {
        return None;
    }
    let after_scheme = url.split_once("://")?.1;
    if after_scheme.is_empty() {
        return None;
    }
    Some((url, close + 1))
}

fn is_url_boundary(text: &str, pos: usize) -> bool {
    if pos == 0 {
        return true;
    }
    let prev = text[..pos].chars().next_back().unwrap();
    !prev.is_alphanumeric() && prev != '_'
}

fn scan_bare_url(slice: &str) -> Option<usize> {
    let scheme_len = if slice.starts_with("https://") {
        8
    } else if slice.starts_with("http://") {
        7
    } else {
        return None;
    };
    let mut end = scheme_len;
    for (idx, ch) in slice[scheme_len..].char_indices() {
        if ch.is_whitespace() || matches!(ch, '<' | '>' | '"' | '`' | '[' | ']') {
            break;
        }
        end = scheme_len + idx + ch.len_utf8();
    }
    while end > scheme_len {
        let last = slice[..end].chars().next_back().unwrap();
        let trim = match last {
            '.' | ',' | ';' | ':' | '!' | '?' | '\'' => true,
            ')' => {
                let candidate = &slice[..end];
                let opens = candidate.bytes().filter(|&b| b == b'(').count();
                let closes = candidate.bytes().filter(|&b| b == b')').count();
                closes > opens
            }
            _ => false,
        };
        if trim {
            end -= last.len_utf8();
        } else {
            break;
        }
    }
    if end > scheme_len { Some(end) } else { None }
}

fn parse_external_link(slice: &str) -> Option<(&str, &str, usize)> {
    let bytes = slice.as_bytes();
    if bytes.first() != Some(&b'[') {
        return None;
    }
    // Scan label up to the matching unescaped `]`, respecting inline code spans.
    let mut pos = 1;
    let mut label_end = None;
    while pos < bytes.len() {
        match bytes[pos] {
            b'\\' if pos + 1 < bytes.len() => pos += 2,
            b'`' => {
                let run_len = bytes[pos..].iter().take_while(|&&b| b == b'`').count();
                if let Some(close_end) = find_closing_backticks(bytes, pos + run_len, run_len) {
                    pos = close_end;
                } else {
                    pos += run_len;
                }
            }
            b'[' => return None,
            b']' => {
                label_end = Some(pos);
                break;
            }
            _ => pos += 1,
        }
    }
    let label_end = label_end?;
    if bytes.get(label_end + 1) != Some(&b'(') {
        return None;
    }
    // Scan destination inside `(...)`, handling balanced parentheses and optional `<...>`.
    let dest_start = label_end + 2;
    let mut dest_pos = dest_start;
    let mut paren_depth = 0usize;
    let mut dest_end = None;
    while dest_pos < bytes.len() {
        match bytes[dest_pos] {
            b'\n' => return None,
            b'\\' if dest_pos + 1 < bytes.len() => dest_pos += 2,
            b'(' => {
                paren_depth += 1;
                dest_pos += 1;
            }
            b')' => {
                if paren_depth == 0 {
                    dest_end = Some(dest_pos);
                    break;
                }
                paren_depth -= 1;
                dest_pos += 1;
            }
            _ => dest_pos += 1,
        }
    }
    let dest_end = dest_end?;
    let dest = slice[dest_start..dest_end].trim();
    if !is_external_link_dest(dest) {
        return None;
    }
    Some((&slice[1..label_end], dest, dest_end + 1))
}

fn is_valid_link_title(title: &str) -> bool {
    let bytes = title.as_bytes();
    if bytes.len() < 2 {
        return false;
    }
    let (open, close) = match (bytes[0], bytes[bytes.len() - 1]) {
        (b'"', b'"') => (b'"', b'"'),
        (b'\'', b'\'') => (b'\'', b'\''),
        (b'(', b')') => (b'(', b')'),
        _ => return false,
    };
    let inner = &bytes[1..bytes.len() - 1];
    let mut i = 0;
    while i < inner.len() {
        if inner[i] == b'\\' {
            if i + 1 == inner.len() {
                // An odd trailing backslash escapes the closing delimiter in CommonMark §6.3.
                return false;
            }
            i += 2;
            continue;
        }
        if inner[i] == close || (open == b'(' && inner[i] == b'(') {
            return false;
        }
        i += 1;
    }
    true
}

fn has_balanced_unescaped_parens(url: &str) -> bool {
    let bytes = url.as_bytes();
    let mut depth = 0usize;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'\\' => {
                if i + 1 < bytes.len() {
                    i += 2;
                } else {
                    i += 1;
                }
            }
            b'(' => {
                depth += 1;
                i += 1;
            }
            b')' => {
                if depth == 0 {
                    return false;
                }
                depth -= 1;
                i += 1;
            }
            _ => i += 1,
        }
    }
    depth == 0
}

fn is_external_link_dest(dest: &str) -> bool {
    let (url, rest) = if let Some(after_lt) = dest.strip_prefix('<') {
        // CommonMark §6.3: find the first unescaped `>`; reject `<`, newlines, and unclosed `\>`.
        let bytes = after_lt.as_bytes();
        let mut close = None;
        let mut i = 0;
        while i < bytes.len() {
            match bytes[i] {
                b'\\' => {
                    if i + 1 == bytes.len() {
                        return false;
                    }
                    i += 2;
                }
                b'<' | b'\n' | b'\r' => return false,
                b'>' => {
                    close = Some(i);
                    break;
                }
                _ => i += 1,
            }
        }
        let Some(close) = close else {
            return false;
        };
        let url = &after_lt[..close];
        if url.contains(|ch: char| ch.is_whitespace() || ch.is_control()) {
            return false;
        }
        let rest = &after_lt[close + 1..];
        if !rest.is_empty() && !rest.as_bytes()[0].is_ascii_whitespace() {
            return false;
        }
        (url, rest.trim_matches(|ch: char| ch.is_ascii_whitespace()))
    } else {
        let split = dest
            .bytes()
            .position(|b| b.is_ascii_whitespace())
            .unwrap_or(dest.len());
        let url = &dest[..split];
        if url.contains(|ch: char| ch.is_whitespace() || ch.is_control() || matches!(ch, '<' | '>'))
            || !has_balanced_unescaped_parens(url)
        {
            return false;
        }
        (
            url,
            dest[split..].trim_matches(|ch: char| ch.is_ascii_whitespace()),
        )
    };
    if !rest.is_empty() && !is_valid_link_title(rest) {
        return false;
    }
    if let Some(after_scheme) = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
    {
        return !after_scheme.is_empty();
    }
    if let Some(after_scheme) = url.strip_prefix("mailto:") {
        return !after_scheme.is_empty();
    }
    false
}

fn render_link_dest(dest: &str, target: Target) -> String {
    if target == Target::Proto
        && let Some(inner) = dest.strip_prefix('<').and_then(|s| s.strip_suffix('>'))
        && (inner.starts_with("http://") || inner.starts_with("https://"))
    {
        return inner.to_string();
    }
    dest.to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_prose_and_real_links_are_unchanged() {
        assert_eq!(
            sanitize_rust_lines("One item submitted to the ingester.\n"),
            vec!["One item submitted to the ingester."]
        );
        assert_eq!(
            sanitize_rust_lines("See `foo` and [docs](https://example.com) for details."),
            vec!["See `foo` and [docs](https://example.com) for details."]
        );
    }

    #[test]
    fn code_blocks_become_closed_text_fences() {
        let input = "Example:\n\n    let x = 1;\n    let y = 2;\n";
        assert_eq!(
            sanitize_rust_lines(input),
            vec!["Example:", "", "```text", "let x = 1;", "let y = 2;", "```"]
        );

        let tab_input = "Example:\n\n\tlet x = 1;\n";
        assert_eq!(
            sanitize_rust_lines(tab_input),
            vec!["Example:", "", "```text", "let x = 1;", "```"]
        );

        let rust_fence = "```rust\nfn bad( {\n```\n";
        assert_eq!(
            sanitize_rust_lines(rust_fence),
            vec!["```text", "fn bad( {", "```"]
        );

        let tilde_fence = "~~~rust\nfn bad( {\n~~~\n";
        assert_eq!(
            sanitize_rust_lines(tilde_fence),
            vec!["~~~text", "fn bad( {", "~~~"]
        );

        let unclosed = "```\nfn bad( {";
        assert_eq!(
            sanitize_rust_lines(unclosed),
            vec!["```text", "fn bad( {", "```"]
        );
    }

    #[test]
    fn html_links_urls_backticks_and_trailing_backslashes_are_escaped() {
        assert_eq!(
            sanitize_rust_lines("list<string> option<T> <br> [tags] [a][b]"),
            vec![r"list\<string\> option\<T\> \<br\> \[tags\] \[a\]\[b\]"]
        );
        assert_eq!(
            sanitize_rust_lines("See https://example.com/a(b)."),
            vec!["See <https://example.com/a(b)>."]
        );
        assert_eq!(
            sanitize_proto_lines("See <https://example.com> and https://example.org."),
            vec!["See https://example.com and https://example.org."]
        );
        assert_eq!(
            sanitize_rust_lines("odd `backtick and path\\"),
            vec![r"odd \`backtick and path\\"]
        );
    }

    #[test]
    fn control_chars_and_block_asterisks_are_normalised() {
        let input = "a\r\nb\rc\0d\u{000B}e\u{000C}f\u{0085}g\u{2028}h\u{2029}i";
        assert_eq!(
            sanitize_rust_lines(input),
            vec!["a", "b", "cd", "e", "f", "g", "h", "i"]
        );

        let block = "\n     * Summary.\n     *\n     *     fn code() {}\n    ";
        assert_eq!(
            sanitize_rust_lines(block),
            vec!["Summary.", "", "```text", "fn code() {}", "```"]
        );

        // Tab-indented block-comment gutter and closing banner lines (`***/`, `/** Hello. **/`).
        let tabbed_block = "\n\t* First line.\n\t*\n\t* Second line.\n\t**\n";
        assert_eq!(
            sanitize_rust_lines(tabbed_block),
            vec!["First line.", "", "Second line."]
        );
        assert_eq!(sanitize_rust_lines("* Hello. *"), vec!["Hello."]);
        assert_eq!(sanitize_rust_lines(" Hello. **"), vec!["Hello."]);
        assert_eq!(sanitize_rust_lines("Pointer T *"), vec!["Pointer T *"]);
        assert_eq!(
            sanitize_rust_lines("Match pattern *"),
            vec!["Match pattern *"]
        );
        assert_eq!(sanitize_rust_lines("**"), Vec::<String>::new());
        assert_eq!(sanitize_rust_lines("***"), Vec::<String>::new());
        assert_eq!(sanitize_rust_lines("* **"), Vec::<String>::new());
        assert_eq!(sanitize_rust_lines("**\n**"), Vec::<String>::new());
        assert_eq!(
            sanitize_rust_lines(" First line.\n * Second line.\n **"),
            vec!["First line.", "Second line."]
        );

        // 1-space-indented `///` Markdown bullet list after a heading must NOT be stripped.
        let markdown_bullets = "Options:\n * alpha\n * beta\n";
        assert_eq!(
            sanitize_rust_lines(markdown_bullets),
            vec!["Options:", " * alpha", " * beta"]
        );
    }

    #[test]
    fn real_wit_parser_block_comments_and_markdown_lists_round_trip() {
        let wit = "\
package ex:docs@0.1.0;

/** Package first line
 * Package second line.
 */
interface api {
  /*** Symmetric single-line banner. ***/
  record one {
    /***\n   * Opened and closed banner.\n   ***/
    a: u32,
    /** Two-line closing banner.\n     ***/
    b: u32,
    /***\n     * Aligned closing banner.\n    ***/
    c: u32,
    /** Inline triple-star banner. ***/
    d: u32,
    /** ***/
    e: u32,
    /** Two-line col-0 closing banner.\n****/
    f: u32,
  }

  /** Two-space indented first line
   * Two-space indented second line.
   */
  record two {
    /// Options:
    ///  * alpha
    ///  * beta
    x: u32,
    /// Items:
    /// * gamma
    /// * delta
    y: u32,
    /// Emphasis **bold** and Pointer T * stay intact.
    z: u32,
    /// Match pattern *
    w: u32,
  }
}
";
        let contract = crate::load_sources(vec![crate::SourceFile::new(
            "contract.wit".to_string(),
            wit.to_string(),
        )])
        .unwrap();
        let model = crate::model::build(&contract).unwrap();
        let iface = &model.interfaces[0];
        assert_eq!(
            sanitize_rust_lines(&iface.docs),
            vec!["Package first line", "Package second line."]
        );
        let one = iface.types.iter().find(|t| t.name == "one").unwrap();
        assert_eq!(
            sanitize_rust_lines(&one.docs),
            vec!["Symmetric single-line banner."]
        );
        let crate::model::TypeDefKind::Record(one_fields) = &one.kind else {
            panic!("expected record");
        };
        assert_eq!(
            sanitize_rust_lines(&one_fields[0].docs),
            vec!["Opened and closed banner."]
        );
        assert_eq!(
            sanitize_rust_lines(&one_fields[1].docs),
            vec!["Two-line closing banner."]
        );
        assert_eq!(
            sanitize_rust_lines(&one_fields[2].docs),
            vec!["Aligned closing banner."]
        );
        assert_eq!(
            sanitize_rust_lines(&one_fields[3].docs),
            vec!["Inline triple-star banner."]
        );
        assert_eq!(
            sanitize_rust_lines(&one_fields[4].docs),
            Vec::<String>::new()
        );
        assert_eq!(
            sanitize_rust_lines(&one_fields[5].docs),
            vec!["Two-line col-0 closing banner."]
        );

        let two = iface.types.iter().find(|t| t.name == "two").unwrap();
        assert_eq!(
            sanitize_rust_lines(&two.docs),
            vec![
                "Two-space indented first line",
                "Two-space indented second line."
            ]
        );
        let crate::model::TypeDefKind::Record(two_fields) = &two.kind else {
            panic!("expected record");
        };
        assert_eq!(
            sanitize_rust_lines(&two_fields[0].docs),
            vec!["Options:", " * alpha", " * beta"]
        );
        assert_eq!(
            sanitize_rust_lines(&two_fields[1].docs),
            vec!["Items:", "* gamma", "* delta"]
        );
        assert_eq!(
            sanitize_rust_lines(&two_fields[2].docs),
            vec!["Emphasis **bold** and Pointer T * stay intact."]
        );
        assert_eq!(
            sanitize_rust_lines(&two_fields[3].docs),
            vec!["Match pattern *"]
        );
    }
}

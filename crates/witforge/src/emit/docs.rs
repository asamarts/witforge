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
fn strip_block_comment_asterisks(lines: &mut [String]) {
    fn parse_star(line: &str) -> Option<(usize, &str)> {
        let spaces = line.bytes().take_while(|&b| b == b' ').count();
        let rest = &line[spaces..];
        let after = rest.strip_prefix('*')?;
        if after.is_empty() {
            Some((spaces, ""))
        } else if let Some(body) = after.strip_prefix(' ') {
            Some((spaces, body))
        } else if after.starts_with('\t') {
            Some((spaces, after))
        } else {
            None
        }
    }

    // Form 1: every non-empty line shares the same `<spaces>*` gutter (`spaces >= 1`), or every
    // non-empty line starts with `*` at column 0 and at least one line is a bare `*` blank line.
    let mut all_match = true;
    let mut gutter: Option<usize> = None;
    let mut has_bare_star = false;
    for line in lines.iter() {
        if line.is_empty() {
            continue;
        }
        let Some((spaces, body)) = parse_star(line) else {
            all_match = false;
            break;
        };
        if body.is_empty() {
            has_bare_star = true;
        }
        match gutter {
            None => gutter = Some(spaces),
            Some(prev) if prev != spaces => {
                all_match = false;
                break;
            }
            Some(_) => {}
        }
    }
    if all_match
        && let Some(spaces) = gutter
        && (spaces >= 1 || has_bare_star)
    {
        for line in lines.iter_mut() {
            if let Some((_, body)) = parse_star(line) {
                *line = body.to_string();
            }
        }
        return;
    }

    // Form 2: `/** First line\n * Second line\n */` where line 0 is prose and every non-empty
    // continuation line shares a `<spaces>*` gutter.
    if lines.len() >= 2 && list_marker_content_col(&lines[0]).is_none() {
        let mut tail_match = true;
        let mut tail_gutter: Option<usize> = None;
        let mut tail_bare_star = false;
        for line in &lines[1..] {
            if line.is_empty() {
                continue;
            }
            let Some((spaces, body)) = parse_star(line) else {
                tail_match = false;
                break;
            };
            if body.is_empty() {
                tail_bare_star = true;
            }
            match tail_gutter {
                None => tail_gutter = Some(spaces),
                Some(prev) if prev != spaces => {
                    tail_match = false;
                    break;
                }
                Some(_) => {}
            }
        }
        if tail_match
            && let Some(spaces) = tail_gutter
            && (spaces == 1 || spaces >= 4 || tail_bare_star)
        {
            lines[0] = lines[0].trim_start_matches(' ').to_string();
            for line in &mut lines[1..] {
                if let Some((_, body)) = parse_star(line) {
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
    let mut list_content_col: Option<usize> = None;
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
            list_content_col = None;
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
        let max_fence_indent = list_content_col.map_or(3, |c| c + 3);
        let is_indented_code = !in_paragraph && col >= 4 && list_content_col.is_none();

        if !is_indented_code
            && let Some((prefix, fence_char, fence_len)) = detect_fence(line, max_fence_indent)
        {
            flush_paragraph(&mut pending_paragraph, &mut out, target);
            in_paragraph = false;
            let expanded_prefix = expand_leading_ws(prefix);
            let fence: String = std::iter::repeat_n(fence_char, fence_len).collect();
            out.push(format!("{expanded_prefix}{fence}text"));
            i += 1;
            let mut closed = false;
            while i < lines.len() {
                if is_closing_fence(&lines[i], fence_char, fence_len, max_fence_indent) {
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

        if !in_paragraph && col >= 4 {
            flush_paragraph(&mut pending_paragraph, &mut out, target);
            list_content_col = None;
            let start = i;
            while i < lines.len() {
                if lines[i].is_empty() || leading_columns(&lines[i]).1 >= 4 {
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
                        strip_columns(code_line, 4)
                    }
                })
                .collect();
            let fence_len = max_backtick_run(&dedented).max(2) + 1;
            let fence = "`".repeat(fence_len);
            out.push(format!("{fence}text"));
            out.extend(dedented);
            out.push(fence);
            continue;
        }

        if is_atx_heading_or_thematic_break(line) {
            flush_paragraph(&mut pending_paragraph, &mut out, target);
            in_paragraph = false;
            list_content_col = None;
            out.extend(escape_paragraph_lines(std::slice::from_ref(line), target));
            i += 1;
            continue;
        }

        if let Some(content_col) = list_marker_content_col(line) {
            flush_paragraph(&mut pending_paragraph, &mut out, target);
            list_content_col = Some(content_col);
            in_paragraph = true;
            pending_paragraph.push(line.clone());
            i += 1;
            continue;
        }

        if col == 0 {
            list_content_col = None;
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

fn list_marker_content_col(line: &str) -> Option<usize> {
    let (byte_off, col) = leading_columns(line);
    if col > 3 {
        return None;
    }
    let rest = &line[byte_off..];
    let bytes = rest.as_bytes();
    if let Some(&first) = bytes.first()
        && matches!(first, b'-' | b'+' | b'*')
    {
        return match bytes.get(1) {
            None | Some(b' ') => Some(col + 2),
            Some(b'\t') => Some(col + 4 - ((col + 1) % 4)),
            _ => None,
        };
    }
    let digits = bytes.iter().take_while(|b| b.is_ascii_digit()).count();
    if (1..=9).contains(&digits) && matches!(bytes.get(digits), Some(b'.' | b')')) {
        let marker_end = col + digits + 1;
        return match bytes.get(digits + 1) {
            None | Some(b' ') => Some(marker_end + 1),
            Some(b'\t') => Some(marker_end + 4 - (marker_end % 4)),
            _ => None,
        };
    }
    None
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

fn is_external_link_dest(dest: &str) -> bool {
    let url = dest.split_whitespace().next().unwrap_or_default();
    let url = url
        .strip_prefix('<')
        .and_then(|s| s.strip_suffix('>'))
        .unwrap_or(url);
    if let Some(rest) = url
        .strip_prefix("https://")
        .or_else(|| url.strip_prefix("http://"))
    {
        return !rest.is_empty();
    }
    if let Some(rest) = url.strip_prefix("mailto:") {
        return !rest.is_empty();
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
    }
}

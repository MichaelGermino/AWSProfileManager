//! Lossless editor for `~/.aws/credentials`. Port of `src/main/services/credentialsIni.ts`,
//! byte-for-byte (vectors: credentials-ini-*.json).
//!
//! - Only the section being written, renamed or removed changes; every other byte stays.
//! - The file keeps its own line endings (CRLF if it has any, else LF); a new file uses the
//!   caller's default.
//! - Section names are literal ("dev.admin" is one section).
//! - Values split at the first "=" only (session tokens end in "=").

use std::sync::LazyLock;

use regex::Regex;

use crate::js;

/// Keys in file order. Later duplicates of a key win (in the first key's position).
pub type Section = Vec<(String, String)>;

static HEADER: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*\[([^\]]*)\]\s*$").unwrap());
static COMMENT: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^\s*[#;]").unwrap());
static HEADER_BRACKETS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\[([^\]]*)\]").unwrap());

fn header_name(line: &str) -> Option<String> {
    HEADER.captures(line).map(|c| js::trim(&c[1]).to_string())
}

fn is_comment(line: &str) -> bool {
    COMMENT.is_match(line)
}

pub fn detect_eol<'a>(text: &str, fallback: &'a str) -> &'a str {
    if text.contains("\r\n") {
        "\r\n"
    } else if text.contains('\n') {
        "\n"
    } else {
        fallback
    }
}

/// `text.split(/\r?\n/)` minus the empty element a trailing newline produces.
fn split_lines(text: &str) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut lines: Vec<String> =
        text.split('\n').map(|l| l.strip_suffix('\r').unwrap_or(l).to_string()).collect();
    if lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    lines
}

/// Sections by name, in first-seen order. Duplicate sections merge.
pub fn parse(text: &str) -> Vec<(String, Section)> {
    let mut out: Vec<(String, Section)> = Vec::new();
    let mut current: Option<usize> = None;
    for line in split_lines(text) {
        if let Some(name) = header_name(&line) {
            let idx = match out.iter().position(|(n, _)| *n == name) {
                Some(i) => i,
                None => {
                    out.push((name, Vec::new()));
                    out.len() - 1
                }
            };
            current = Some(idx);
            continue;
        }
        let Some(idx) = current else { continue };
        if is_comment(&line) || !line.contains('=') {
            continue;
        }
        let eq = line.find('=').unwrap();
        let key = js::trim(&line[..eq]);
        if key.is_empty() {
            continue;
        }
        let value = js::trim(&line[eq + 1..]).to_string();
        let section = &mut out[idx].1;
        match section.iter_mut().find(|(k, _)| k == key) {
            Some(slot) => slot.1 = value,
            None => section.push((key.to_string(), value)),
        }
    }
    out
}

pub fn get<'a>(parsed: &'a [(String, Section)], name: &str) -> Option<&'a Section> {
    parsed.iter().find(|(n, _)| n == name).map(|(_, s)| s)
}

/// Comment lines directly above a header belong to its block, then the header, its keys, and the
/// blank / comment lines that follow until the next block.
struct Block {
    name: Option<String>,
    lines: Vec<String>,
    /// Index of the header in `lines`; `None` for the lines before the first section.
    header_at: Option<usize>,
}

fn blocks(lines: Vec<String>) -> Vec<Block> {
    let mut out = vec![Block { name: None, lines: Vec::new(), header_at: None }];
    for line in lines {
        let Some(name) = header_name(&line) else {
            out.last_mut().unwrap().lines.push(line);
            continue;
        };
        let prev = out.last_mut().unwrap();
        let floor = prev.header_at.map_or(0, |h| h + 1);
        let mut k = prev.lines.len();
        while k > floor && is_comment(&prev.lines[k - 1]) {
            k -= 1;
        }
        let mut lines: Vec<String> = prev.lines.split_off(k);
        let header_at = lines.len();
        lines.push(line);
        out.push(Block { name: Some(name), lines, header_at: Some(header_at) });
    }
    out
}

/// Index just past the block's last key line; trailing blanks and comments are its tail.
fn content_end(b: &Block) -> usize {
    let floor = b.header_at.map_or(0, |h| h + 1);
    let mut end = b.lines.len();
    while end > floor && (js::trim(&b.lines[end - 1]).is_empty() || is_comment(&b.lines[end - 1])) {
        end -= 1;
    }
    end
}

/// No blank lines dangling at the start or end of the file.
fn tidy_edges(lines: Vec<String>) -> Vec<String> {
    let blank = |l: &String| js::trim(l).is_empty();
    let start = lines.iter().position(|l| !blank(l)).unwrap_or(lines.len());
    let end = lines.iter().rposition(|l| !blank(l)).map_or(start, |i| i + 1);
    lines[start..end.max(start)].to_vec()
}

fn join(lines: &[String], eol: &str) -> String {
    if lines.is_empty() {
        String::new()
    } else {
        let mut s = lines.join(eol);
        s.push_str(eol);
        s
    }
}

/// Write (replace or append) one section, keys as `key=value` in the given order.
pub fn set_section(text: &str, name: &str, values: &[(String, String)], default_eol: &str) -> String {
    let eol = detect_eol(text, default_eol);
    let body: Vec<String> = values.iter().map(|(k, v)| format!("{k}={v}")).collect();
    let mut all = blocks(split_lines(text));
    if let Some(ti) = all.iter().position(|b| b.name.as_deref() == Some(name)) {
        let target = &mut all[ti];
        let head_end = target.header_at.map_or(0, |h| h + 1);
        let tail: Vec<String> = target.lines[content_end(target)..].to_vec();
        let mut lines: Vec<String> = target.lines[..head_end].to_vec();
        lines.extend(body);
        lines.extend(tail);
        target.lines = lines;
        // Later sections with the same name would shadow this one in other tools: drop them.
        let total = all.len();
        let rest: Vec<Block> =
            all.into_iter().enumerate().filter(|(i, b)| *i == ti || b.name.as_deref() != Some(name)).map(|(_, b)| b).collect();
        let lines: Vec<String> = rest.iter().flat_map(|b| b.lines.iter().cloned()).collect();
        return join(&if rest.len() == total { lines } else { tidy_edges(lines) }, eol);
    }
    let mut lines: Vec<String> = all.into_iter().flat_map(|b| b.lines).collect();
    if lines.last().is_some_and(|l| !js::trim(l).is_empty()) {
        lines.push(String::new());
    }
    lines.push(format!("[{name}]"));
    lines.extend(body);
    join(&lines, eol)
}

/// Rename a section's header in place; keys, comments and position stay. `None` if absent.
pub fn rename_section(text: &str, from: &str, to: &str, default_eol: &str) -> Option<String> {
    let eol = detect_eol(text, default_eol);
    let mut all = blocks(split_lines(text));
    let target = all.iter_mut().find(|b| b.name.as_deref() == Some(from))?;
    if from == to {
        return None;
    }
    let h = target.header_at.unwrap();
    // `String.replace` with a string pattern: "$" sequences in `to` are special in JS. Section
    // names never contain them in practice; NoExpand keeps Rust literal.
    target.lines[h] = HEADER_BRACKETS.replace(&target.lines[h], regex::NoExpand(&format!("[{to}]"))).into_owned();
    target.name = Some(to.to_string());
    Some(join(&all.into_iter().flat_map(|b| b.lines).collect::<Vec<_>>(), eol))
}

/// Remove a section with the comments directly above it and its trailing lines. `None` if absent.
pub fn remove_section(text: &str, name: &str, default_eol: &str) -> Option<String> {
    let eol = detect_eol(text, default_eol);
    let all = blocks(split_lines(text));
    if !all.iter().any(|b| b.name.as_deref() == Some(name)) {
        return None;
    }
    let kept: Vec<String> = all.into_iter().filter(|b| b.name.as_deref() != Some(name)).flat_map(|b| b.lines).collect();
    Some(join(&tidy_edges(kept), eol))
}

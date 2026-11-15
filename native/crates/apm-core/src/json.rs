//! JSON files exactly as 1.x writes and reads them.
//!
//! 1.x writes every store with `JSON.stringify(x, null, 2)`: two-space indent, no trailing newline,
//! keys in insertion order, numbers in JavaScript's shortest form (`60`, never `60.0`). A 1.x build
//! reinstalled over 2.0 data must find byte-identical files, and the scenarios compare text.
//!
//! Reading matches `JSON.parse` inside a try/catch: a missing or malformed file reads as `None`,
//! and nothing is ever rewritten just because it was read.

use std::fmt::Write as _;
use std::path::Path;

use serde_json::Value;

/// `JSON.stringify(value, null, 2)`.
pub fn stringify_pretty(value: &Value) -> String {
    let mut out = String::new();
    write_value(&mut out, value, 0);
    out
}

/// `JSON.stringify(value)` (compact).
pub fn stringify(value: &Value) -> String {
    let mut out = String::new();
    write_compact(&mut out, value);
    out
}

fn write_value(out: &mut String, value: &Value, depth: usize) {
    match value {
        Value::Array(items) if !items.is_empty() => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                out.push_str(if i == 0 { "\n" } else { ",\n" });
                indent(out, depth + 1);
                write_value(out, item, depth + 1);
            }
            out.push('\n');
            indent(out, depth);
            out.push(']');
        }
        Value::Object(map) if !map.is_empty() => {
            out.push('{');
            for (i, (k, v)) in map.iter().enumerate() {
                out.push_str(if i == 0 { "\n" } else { ",\n" });
                indent(out, depth + 1);
                write_string(out, k);
                out.push_str(": ");
                write_value(out, v, depth + 1);
            }
            out.push('\n');
            indent(out, depth);
            out.push('}');
        }
        other => write_compact(out, other),
    }
}

fn write_compact(out: &mut String, value: &Value) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => write_number(out, n),
        Value::String(s) => write_string(out, s),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_compact(out, item);
            }
            out.push(']');
        }
        Value::Object(map) => {
            out.push('{');
            for (i, (k, v)) in map.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_string(out, k);
                out.push(':');
                write_compact(out, v);
            }
            out.push('}');
        }
    }
}

fn indent(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push_str("  ");
    }
}

/// JavaScript `Number.prototype.toString` for finite values; non-finite numbers are `null` in JSON.
fn write_number(out: &mut String, n: &serde_json::Number) {
    if let Some(i) = n.as_i64() {
        let _ = write!(out, "{i}");
    } else if let Some(u) = n.as_u64() {
        let _ = write!(out, "{u}");
    } else if let Some(f) = n.as_f64() {
        if f.is_finite() {
            out.push_str(ryu_js::Buffer::new().format_finite(f));
        } else {
            out.push_str("null");
        }
    }
}

/// `JSON.stringify` string escaping: `"`, `\`, the short escapes, other C0 controls as `\u00xx`.
/// Everything else (non-ASCII, `/`, U+2028) is written as-is.
fn write_string(out: &mut String, s: &str) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// The parsed file, or `None` when it is missing or not valid JSON.
pub fn read_file(path: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(path).ok()?;
    serde_json::from_str(&text).ok()
}

/// Write `stringify_pretty(value)`, creating the parent directory if needed.
pub fn write_file(path: &Path, value: &Value) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, stringify_pretty(value))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn matches_json_stringify() {
        let v = json!({"a": 1, "b": [1.5, 60.0, -0.0, 1e21], "c": {}, "d": [], "e": "q\"\n\u{1}é/"});
        assert_eq!(
            stringify_pretty(&v),
            "{\n  \"a\": 1,\n  \"b\": [\n    1.5,\n    60,\n    0,\n    1e+21\n  ],\n  \"c\": {},\n  \"d\": [],\n  \"e\": \"q\\\"\\n\\u0001é/\"\n}"
        );
    }
}

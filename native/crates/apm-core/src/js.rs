//! Small helpers that reproduce JavaScript string semantics the 1.x code relies on, so ports stay
//! byte-compatible without each module re-deriving the edge cases.

/// `String.prototype.trim()`: strips ECMAScript whitespace and line terminators, which includes
/// U+FEFF (a BOM) — Rust's `str::trim` does not.
pub fn trim(s: &str) -> &str {
    s.trim_matches(is_js_whitespace)
}

pub fn is_js_whitespace(c: char) -> bool {
    c.is_whitespace() || c == '\u{feff}'
}

/// JavaScript truthiness for a JSON value (`if (value)`): `null`, `false`, `0` and `""` are falsy.
pub fn truthy(v: &serde_json::Value) -> bool {
    use serde_json::Value;
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}

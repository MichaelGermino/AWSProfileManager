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

/// JavaScript truthiness for an optional string after `?.trim()`: `None` and `""` are falsy.
pub fn non_empty(s: Option<&str>) -> Option<&str> {
    s.filter(|v| !v.is_empty())
}

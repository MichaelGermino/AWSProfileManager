//! `a.localeCompare(b)` as V8 does it with the default (root) ICU collation, for the strings this
//! area sorts: Open WebUI model ids and AWS CLI service slugs.
//!
//! Not a full UCA implementation. It reproduces the parts that change the order of ASCII-ish ids:
//! - primary level: whitespace < punctuation < symbols < digits < letters, letters case-blind
//!   (so `model-B` sorts with `model-b`, and `Z` after `a` — byte order would put `Z` first);
//! - punctuation is NOT ignored (`Intl.Collator` default `ignorePunctuation: false`);
//! - tertiary level: lowercase before uppercase at the first case difference.
//!
//! Other characters sort after the ASCII letters by their lowercased code point.

use std::cmp::Ordering;

/// ASCII punctuation and symbols in DUCET (root collation) order.
const PUNCT_ORDER: &str = "_-,;:!?.'\"()[]{}@*/\\&#%`^+<=>|~$";

fn primary(c: char) -> (u32, u32) {
    if c.is_whitespace() {
        return (0, c as u32);
    }
    if let Some(i) = PUNCT_ORDER.find(c) {
        return (1, i as u32);
    }
    if c.is_ascii_digit() {
        return (2, c as u32);
    }
    if c.is_ascii_alphabetic() {
        return (3, c.to_ascii_lowercase() as u32);
    }
    let lower = c.to_lowercase().next().unwrap_or(c);
    (4, lower as u32)
}

pub fn locale_compare(a: &str, b: &str) -> Ordering {
    let pa: Vec<_> = a.chars().map(primary).collect();
    let pb: Vec<_> = b.chars().map(primary).collect();
    match pa.cmp(&pb) {
        Ordering::Equal => {}
        o => return o,
    }
    // Tertiary: lowercase first.
    for (x, y) in a.chars().zip(b.chars()) {
        if x != y {
            let (xl, yl) = (x.is_lowercase(), y.is_lowercase());
            if xl && !yl {
                return Ordering::Less;
            }
            if yl && !xl {
                return Ordering::Greater;
            }
            return x.cmp(&y);
        }
    }
    a.len().cmp(&b.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn like_v8_locale_compare() {
        let mut v = vec!["model-b", "Zeta", "alpha", "model-a", "Model-a", "s3api", "s3-outposts", "s3", "gpt4", "gpt-4o", "10", "9"];
        v.sort_by(|a, b| locale_compare(a, b));
        assert_eq!(v, ["10", "9", "alpha", "gpt-4o", "gpt4", "model-a", "Model-a", "model-b", "s3", "s3-outposts", "s3api", "Zeta"]);
    }
}

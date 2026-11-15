//! Master password rules. Port of `src/shared/masterPassword.ts` (NIST SP 800-63: length only,
//! no composition rules).

pub const MIN_LENGTH: usize = 8;
/// NIST allows up to 64; 128 leaves room for passphrases.
pub const MAX_LENGTH: usize = 128;

pub const REQUIREMENTS: &str = "Use at least 8 characters. Longer passphrases are stronger.";

/// Length in characters (Unicode code points): four "🔑" are four characters, not eight.
pub fn length(password: &str) -> usize {
    password.chars().count()
}

/// The error message for an invalid password, or `None` when it is acceptable.
pub fn validate(password: &str) -> Option<String> {
    let n = length(password);
    if n < MIN_LENGTH {
        return Some(format!("Password must be at least {MIN_LENGTH} characters."));
    }
    if n > MAX_LENGTH {
        return Some(format!("Password must be no more than {MAX_LENGTH} characters."));
    }
    None
}

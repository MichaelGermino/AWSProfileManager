//! Credential state, shown the same way everywhere: a status dot plus a mono countdown. The app's
//! one signature detail; port of `src/renderer/components/CredentialStatus.tsx`.

use serde_json::Value;

/// Under this many seconds left, an active credential reads as "expiring" (amber).
pub const EXPIRING_SOON_SECONDS: i64 = 15 * 60;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CredState {
    Active,
    Expiring,
    Expired,
    Never,
}

impl CredState {
    /// From a `dashboard:getState` summary.
    pub fn of(summary: &Value) -> Self {
        let remaining = summary.get("timeRemainingSeconds").and_then(Value::as_i64);
        match summary.get("status").and_then(Value::as_str) {
            Some("expired") => CredState::Expired,
            Some("active") if remaining.is_some_and(|r| r < EXPIRING_SOON_SECONDS) => CredState::Expiring,
            Some("active") => CredState::Active,
            _ => CredState::Never,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            CredState::Active => "Active",
            CredState::Expiring => "Expiring soon",
            CredState::Expired => "Expired",
            CredState::Never => "Not signed in",
        }
    }

    /// Signed out or expired: the row's one-click action is sign-in instead of the console.
    pub fn needs_sign_in(self) -> bool {
        matches!(self, CredState::Expired | CredState::Never)
    }
}

/// "2d 3h", "4h 05m", "47m", "<1m". Days appear past 48 hours.
pub fn format_countdown(seconds: i64) -> String {
    if seconds < 60 {
        return "<1m".into();
    }
    let total_hours = seconds / 3600;
    let m = (seconds % 3600) / 60;
    if total_hours >= 48 {
        return format!("{}d {}h", seconds / 86400, (seconds % 86400) / 3600);
    }
    if total_hours > 0 {
        return format!("{total_hours}h {m:02}m");
    }
    format!("{m}m")
}

/// Short text for a row: the countdown while active, otherwise a word.
pub fn short_label(state: CredState, remaining: Option<i64>) -> String {
    match state {
        CredState::Expired => "expired".into(),
        CredState::Never => "not signed in".into(),
        _ => remaining.map(|s| format!("{} left", format_countdown(s))).unwrap_or_else(|| "active".into()),
    }
}

/// Main sends "12/31/2098, 4:00:00 PM" (Pacific). Drop the seconds and name the zone.
pub fn format_expiry(expires_at_pst: &str) -> String {
    let trimmed = match expires_at_pst.rfind(':') {
        Some(i) if expires_at_pst[i + 1..].len() >= 2 && expires_at_pst[i + 1..i + 3].chars().all(|c| c.is_ascii_digit()) => {
            format!("{}{}", &expires_at_pst[..i], &expires_at_pst[i + 3..])
        }
        _ => expires_at_pst.to_string(),
    };
    format!("{trimmed} PT")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn countdowns() {
        assert_eq!(format_countdown(30), "<1m");
        assert_eq!(format_countdown(47 * 60), "47m");
        assert_eq!(format_countdown(4 * 3600 + 5 * 60), "4h 05m");
        assert_eq!(format_countdown(50 * 3600), "2d 2h");
        assert_eq!(format_expiry("12/31/2098, 4:00:00\u{202f}PM"), "12/31/2098, 4:00\u{202f}PM PT");
    }
}

//! Which profile the Terminal screen starts on. Port of `src/shared/terminalProfile.ts`.

/// `default_id` is `Settings.defaultTerminalProfileId`:
/// - `None`: never chosen, so the first profile is used;
/// - `Some("")`: explicitly "No profile";
/// - `Some(id)`: that profile, or the first one if it no longer exists.
///
/// Returns the profile id to select, or `None` for "No profile".
pub fn resolve_terminal_profile_id<'a>(default_id: Option<&'a str>, profile_ids: &[&'a str]) -> Option<&'a str> {
    let first = *profile_ids.first()?;
    match default_id {
        None => Some(first),
        Some("") => None,
        Some(id) if profile_ids.contains(&id) => Some(id),
        Some(_) => Some(first),
    }
}

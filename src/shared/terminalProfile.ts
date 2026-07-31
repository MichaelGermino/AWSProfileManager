/**
 * Resolves which profile the Terminal screen should start on.
 *
 * Shared by the Terminal screen (which applies it) and Settings (which displays it),
 * so the two can't drift — the dropdown always shows what the terminal will actually do.
 */

/** Minimal shape needed here; accepts a full Profile without depending on it. */
interface ProfileLike {
  id: string;
}

/**
 * @param defaultTerminalProfileId Settings.defaultTerminalProfileId — see its doc comment
 *        for why undefined and '' mean different things.
 * @param profiles Available profiles, in display order.
 * @returns The profile id to select, or null for "No profile".
 */
export function resolveTerminalProfileId(
  defaultTerminalProfileId: string | undefined,
  profiles: readonly ProfileLike[]
): string | null {
  if (profiles.length === 0) return null;

  // Never chosen: default to the first available profile.
  if (defaultTerminalProfileId === undefined) return profiles[0].id;

  // Explicitly "No profile" — respect it rather than helpfully picking one.
  if (defaultTerminalProfileId === '') return null;

  // Chosen, but the profile may since have been deleted or restored from a backup.
  const stillExists = profiles.some((p) => p.id === defaultTerminalProfileId);
  return stillExists ? defaultTerminalProfileId : profiles[0].id;
}

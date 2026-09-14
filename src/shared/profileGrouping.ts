/**
 * Partitions profiles into folders for display.
 *
 * Shared by the Profiles page and (later) the tray and terminal picker, so the grouping can't
 * drift between them. Must stay free of Node built-ins — the renderer imports it.
 *
 * The flat profile array remains the canonical order: folders are a render-time partition, which
 * is what lets the tray menu, resolveTerminalProfileId() and every `profiles[0]` fallback keep
 * working untouched.
 *
 * A profile whose folderId names a folder that no longer exists falls into `ungrouped` rather
 * than vanishing. That is the self-heal for a dangling id — a restore from backup, a crash
 * between the two writes that deleting a folder performs, or a file edited by hand. No migration
 * needed: the next drag that touches the profile rewrites folderId anyway.
 */

import type { ProfileFolder } from './types';

/** Minimal shape needed here; accepts a full Profile or a DashboardProfileSummary. */
interface FolderedLike {
  folderId?: string;
}

export interface FolderGroup<T> {
  folder: ProfileFolder;
  profiles: T[];
}

export interface GroupedProfiles<T> {
  /** In `folders` order. Every folder appears, including empty ones. */
  groups: FolderGroup<T>[];
  /** Profiles with no folder, in profile-array order. */
  ungrouped: T[];
}

export function groupProfilesByFolder<T extends FolderedLike>(
  profiles: readonly T[],
  folders: readonly ProfileFolder[]
): GroupedProfiles<T> {
  const groups: FolderGroup<T>[] = folders.map((folder) => ({ folder, profiles: [] }));
  const byFolderId = new Map(groups.map((g) => [g.folder.id, g]));
  const ungrouped: T[] = [];

  for (const profile of profiles) {
    const group = profile.folderId ? byFolderId.get(profile.folderId) : undefined;
    if (group) group.profiles.push(profile);
    else ungrouped.push(profile);
  }

  return { groups, ungrouped };
}

/**
 * Flatten a grouping back to the canonical profile order: folders first (in folder order, each
 * folder's members contiguous), then ungrouped. Used after a drag to build the id list that
 * `profiles:applyLayout` persists.
 */
export function flattenGrouping<T extends FolderedLike>(grouped: GroupedProfiles<T>): T[] {
  return [...grouped.groups.flatMap((g) => g.profiles), ...grouped.ungrouped];
}

import crypto from 'crypto';
import fs from 'fs';
import path from 'path';
import type { ProfileFolder } from '../../shared/types';
import { getAppDataPath, clearFolderAssignments } from './profileStorage';

/**
 * Folder definitions, in their own file — deliberately NOT inside profiles.json.
 *
 * profiles.json is read-modify-written by every build of this app. An older build's
 * writeProfilesData() serializes `{ profiles }` and nothing else, so a key it doesn't know about
 * is dropped on the next profile save, delete, reorder, or scheduler expiration update. A user
 * who installs an older version after making folders would lose every folder name and color.
 * A separate file is one an old build cannot touch, so a downgrade-then-upgrade round trip
 * leaves folders intact. Membership (`Profile.folderId`) rides along inside the profile object,
 * where normalizeProfile's spread preserves it across versions.
 *
 * The array order IS the folder display order, mirroring how profiles.json works.
 */

const FOLDERS_FILENAME = 'folders.json';

interface FoldersData {
  folders: ProfileFolder[];
}

function getFoldersPath(): string {
  return path.join(getAppDataPath(), FOLDERS_FILENAME);
}

function ensureAppDataDir(): void {
  const dir = getAppDataPath();
  if (!fs.existsSync(dir)) {
    fs.mkdirSync(dir, { recursive: true });
  }
}

/** Drop anything that isn't a usable folder, so one bad entry can't blank the whole list. */
function normalizeFolder(raw: unknown): ProfileFolder | null {
  if (!raw || typeof raw !== 'object') return null;
  const f = raw as Partial<ProfileFolder>;
  if (typeof f.id !== 'string' || !f.id.trim()) return null;
  return {
    id: f.id,
    name: typeof f.name === 'string' ? f.name : '',
    ...(typeof f.color === 'string' ? { color: f.color } : {}),
    ...(typeof f.iconName === 'string' ? { iconName: f.iconName } : {}),
  };
}

function readFoldersData(): FoldersData {
  ensureAppDataDir();
  const filePath = getFoldersPath();
  if (!fs.existsSync(filePath)) {
    return { folders: [] };
  }
  try {
    const raw = fs.readFileSync(filePath, 'utf-8');
    const data = JSON.parse(raw) as { folders?: unknown[] };
    const folders = Array.isArray(data.folders)
      ? data.folders.map(normalizeFolder).filter((f): f is ProfileFolder => f !== null)
      : [];
    return { folders };
  } catch {
    // Same contract as profiles/settings: a malformed file reads as empty, never throws.
    return { folders: [] };
  }
}

function writeFoldersData(data: FoldersData): void {
  ensureAppDataDir();
  fs.writeFileSync(getFoldersPath(), JSON.stringify(data, null, 2), 'utf-8');
}

export function getFolders(): ProfileFolder[] {
  return readFoldersData().folders;
}

/** Longer than this and the folder header truncates anyway. */
const MAX_NAME_LENGTH = 40;

function cleanName(name: string | undefined): string {
  return (name ?? '').trim().slice(0, MAX_NAME_LENGTH);
}

/**
 * Create (no id, or an id that no longer exists) or update a folder. Returns the stored folder so
 * the renderer learns the generated id.
 *
 * A blank name is rejected rather than stored: on create it becomes 'New folder', and on update
 * the existing name is kept, so a rename that submits empty is a no-op instead of an unnameable
 * folder the user can only recover from by deleting.
 */
export function saveFolder(input: Partial<ProfileFolder>): ProfileFolder {
  const data = readFoldersData();
  const index = input.id ? data.folders.findIndex((f) => f.id === input.id) : -1;

  if (index >= 0) {
    const existing = data.folders[index];
    const updated: ProfileFolder = {
      ...existing,
      name: cleanName(input.name) || existing.name,
      ...(input.color !== undefined ? { color: input.color } : {}),
      ...(input.iconName !== undefined ? { iconName: input.iconName } : {}),
    };
    data.folders[index] = updated;
    writeFoldersData(data);
    return updated;
  }

  const created: ProfileFolder = {
    id: input.id ?? crypto.randomUUID(),
    name: cleanName(input.name) || 'New folder',
    ...(input.color !== undefined ? { color: input.color } : {}),
    ...(input.iconName !== undefined ? { iconName: input.iconName } : {}),
  };
  data.folders.push(created);
  writeFoldersData(data);
  return created;
}

/**
 * Delete a folder. Its profiles are never deleted — they become ungrouped.
 *
 * Two files change, and they are written in the order that fails safe: profiles first, so a crash
 * in between leaves a folder with no members rather than profiles pointing at a folder that no
 * longer exists. Either way groupProfilesByFolder() renders the result correctly.
 */
export function deleteFolder(id: string): void {
  clearFolderAssignments(id);
  const data = readFoldersData();
  data.folders = data.folders.filter((f) => f.id !== id);
  writeFoldersData(data);
}

/** Reorder folders by id list; unknown ids are dropped, missing ones appended. */
export function reorderFolders(orderedIds: string[]): void {
  const data = readFoldersData();
  const byId = new Map(data.folders.map((f) => [f.id, f]));
  const ordered = orderedIds.filter((fid) => byId.has(fid)).map((fid) => byId.get(fid)!);
  const seen = new Set(ordered.map((f) => f.id));
  const rest = data.folders.filter((f) => !seen.has(f.id));
  data.folders = [...ordered, ...rest];
  writeFoldersData(data);
}

/** Replace all folders (used when restoring from backup). */
export function replaceAllFolders(folders: ProfileFolder[]): void {
  const normalized = Array.isArray(folders)
    ? folders.map(normalizeFolder).filter((f): f is ProfileFolder => f !== null)
    : [];
  writeFoldersData({ folders: normalized });
}

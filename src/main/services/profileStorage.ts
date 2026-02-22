import { app } from 'electron';
import fs from 'fs';
import path from 'path';
import type { Profile } from '../../shared/types';
import { removeCredentialsSection } from './credentialsFile';

const APP_NAME = 'AWSProfileManager';

export function getAppDataPath(): string {
  const base = process.env.APPDATA || path.join(app.getPath('home'), 'AppData', 'Roaming');
  return path.join(base, APP_NAME);
}

function getProfilesPath(): string {
  return path.join(getAppDataPath(), 'profiles.json');
}

function ensureAppDataDir(): void {
  const dir = getAppDataPath();
  if (!fs.existsSync(dir)) {
    fs.mkdirSync(dir, { recursive: true });
  }
}

interface ProfilesData {
  profiles: Profile[];
}

/** Migrate legacy refreshIntervalHours to refreshIntervalMinutes. */
function normalizeProfile(raw: Record<string, unknown>): Profile {
  const p = raw as Partial<Profile>;
  const intervalMinutes =
    typeof p.refreshIntervalMinutes === 'number'
      ? p.refreshIntervalMinutes
      : (typeof (raw as { refreshIntervalHours?: number }).refreshIntervalHours === 'number'
          ? (raw as { refreshIntervalHours: number }).refreshIntervalHours * 60
          : 60);
  return {
    ...p,
    refreshIntervalMinutes: Math.max(1, Math.floor(intervalMinutes)),
  } as Profile;
}

function readProfilesData(): ProfilesData {
  ensureAppDataDir();
  const filePath = getProfilesPath();
  if (!fs.existsSync(filePath)) {
    return { profiles: [] };
  }
  try {
    const raw = fs.readFileSync(filePath, 'utf-8');
    const data = JSON.parse(raw) as { profiles?: Record<string, unknown>[] };
    const profiles = Array.isArray(data.profiles)
      ? data.profiles.map(normalizeProfile)
      : [];
    return { profiles };
  } catch {
    return { profiles: [] };
  }
}

function writeProfilesData(data: ProfilesData): void {
  ensureAppDataDir();
  fs.writeFileSync(getProfilesPath(), JSON.stringify(data, null, 2), 'utf-8');
}

export function getProfiles(): Profile[] {
  return readProfilesData().profiles;
}

export function saveProfile(profile: Profile): void {
  const data = readProfilesData();
  const index = data.profiles.findIndex((p) => p.id === profile.id);
  if (index >= 0) {
    data.profiles[index] = profile;
  } else {
    data.profiles.push(profile);
  }
  writeProfilesData(data);
}

export function deleteProfile(id: string): void {
  const data = readProfilesData();
  const profile = data.profiles.find((p) => p.id === id);
  if (profile) {
    const sectionName = (profile.credentialProfileName || profile.name || '').trim();
    if (sectionName) removeCredentialsSection(sectionName);
  }
  data.profiles = data.profiles.filter((p) => p.id !== id);
  writeProfilesData(data);
}

/** Reorder profiles by id list; writes back to storage. Unknown ids are appended. */
export function reorderProfiles(orderedIds: string[]): void {
  const data = readProfilesData();
  const byId = new Map(data.profiles.map((p) => [p.id, p]));
  const ordered = orderedIds
    .filter((id) => byId.has(id))
    .map((id) => byId.get(id)!);
  const rest = data.profiles.filter((p) => !orderedIds.includes(p.id));
  data.profiles = [...ordered, ...rest];
  writeProfilesData(data);
}

export function getProfileById(id: string): Profile | null {
  return getProfiles().find((p) => p.id === id) ?? null;
}

/** Replace all profiles with the given list (used when restoring from backup). */
export function replaceAllProfiles(profiles: Profile[]): void {
  ensureAppDataDir();
  const normalized = Array.isArray(profiles) ? profiles.map((p) => normalizeProfile(p as unknown as Record<string, unknown>)) : [];
  writeProfilesData({ profiles: normalized });
}

import fs from 'fs';
import path from 'path';
import { getAppDataPath } from './profileStorage';
import type { AwsRole } from '../../shared/types';

function getRolesCachePath(): string {
  return path.join(getAppDataPath(), 'rolesCache.json');
}

interface RolesCacheEntry {
  roles: AwsRole[];
  fetchedAt: number;
}

interface RolesCacheFile {
  [idpEntryUrl: string]: RolesCacheEntry;
}

function readCache(): RolesCacheFile {
  const filePath = getRolesCachePath();
  const dir = path.dirname(filePath);
  if (!fs.existsSync(dir)) {
    try {
      fs.mkdirSync(dir, { recursive: true });
    } catch {
      return {};
    }
  }
  if (!fs.existsSync(filePath)) return {};
  try {
    const raw = fs.readFileSync(filePath, 'utf-8');
    const data = JSON.parse(raw) as RolesCacheFile;
    return typeof data === 'object' && data !== null ? data : {};
  } catch {
    return {};
  }
}

function writeCache(data: RolesCacheFile): void {
  const filePath = getRolesCachePath();
  const dir = path.dirname(filePath);
  if (!fs.existsSync(dir)) {
    fs.mkdirSync(dir, { recursive: true });
  }
  fs.writeFileSync(filePath, JSON.stringify(data, null, 2), 'utf-8');
}

export function getCachedRoles(idpEntryUrl: string): AwsRole[] | null {
  if (!idpEntryUrl?.trim()) return null;
  const cache = readCache();
  const entry = cache[idpEntryUrl];
  return entry?.roles ?? null;
}

export function setCachedRoles(idpEntryUrl: string, roles: AwsRole[]): void {
  if (!idpEntryUrl?.trim()) return;
  const cache = readCache();
  cache[idpEntryUrl] = { roles, fetchedAt: Date.now() };
  writeCache(cache);
}

import fs from 'fs';
import path from 'path';
import { app } from 'electron';
import type { Profile } from '../../shared/types';
import { notifyAuthAuditLogChanged } from './authLogViewer';
import { getProfiles } from './profileStorage';
import { getSettings } from './settingsService';

const APP_NAME = 'AWSProfileManager';
const LOG_FILE = 'auth-audit-log.json';
const RETENTION_MS = 5 * 24 * 60 * 60 * 1000;

export type AuthAuditEntry = {
  t: number;
  type: 'idp_request' | 'idp_success' | 'failure';
  /** e.g. refreshProfile, fetchRolesForIdp, performLogin */
  source: string;
  profileId?: string;
  idpHost?: string;
  /** First character only + mask */
  usernameHint?: string;
  error?: string;
  /** idp_success: roles parsed from SAML assertion */
  roleCount?: number;
  /** idp_success: ms from IdP POST to SAML parsed */
  durationMs?: number;
};

/** Resolved from current profiles when serving the log viewer (not persisted in the log file). */
export type AuthAuditEntryForViewer = AuthAuditEntry & { profileName?: string };

function profileDisplayName(p: Profile): string {
  const n = p.name?.trim();
  if (n) return n;
  const l = p.label?.trim();
  if (l) return l;
  return p.credentialProfileName?.trim() || '';
}

function getAppDataPath(): string {
  const base = process.env.APPDATA || path.join(app.getPath('home'), 'AppData', 'Roaming');
  return path.join(base, APP_NAME);
}

function getLogPath(): string {
  return path.join(getAppDataPath(), LOG_FILE);
}

function ensureDir(): void {
  const dir = getAppDataPath();
  if (!fs.existsSync(dir)) fs.mkdirSync(dir, { recursive: true });
}

export function isAuthLoggingEnabled(): boolean {
  const s = getSettings();
  return s.authLoggingEnabled !== false;
}

function prune(entries: AuthAuditEntry[]): AuthAuditEntry[] {
  const cutoff = Date.now() - RETENTION_MS;
  return entries.filter((e) => e.t >= cutoff);
}

function readEntries(): AuthAuditEntry[] {
  const p = getLogPath();
  if (!fs.existsSync(p)) return [];
  try {
    const raw = fs.readFileSync(p, 'utf-8');
    const data = JSON.parse(raw) as unknown;
    if (!Array.isArray(data)) return [];
    return data as AuthAuditEntry[];
  } catch {
    return [];
  }
}

function writeEntries(entries: AuthAuditEntry[]): void {
  ensureDir();
  fs.writeFileSync(getLogPath(), JSON.stringify(entries, null, 0), 'utf-8');
}

export function appendAuthAudit(entry: Omit<AuthAuditEntry, 't'>): void {
  if (!isAuthLoggingEnabled()) return;
  const full: AuthAuditEntry = { ...entry, t: Date.now() };
  const merged = prune(readEntries());
  merged.push(full);
  writeEntries(prune(merged));
  notifyAuthAuditLogChanged();
}

/** Newest first, for the log viewer UI. Includes `profileName` when the id still exists in profiles. */
export function getAuthAuditEntriesForViewer(): AuthAuditEntryForViewer[] {
  const byId = new Map<string, string>();
  for (const p of getProfiles()) {
    const d = profileDisplayName(p);
    if (d) byId.set(p.id, d);
  }
  return prune(readEntries())
    .sort((a, b) => b.t - a.t)
    .map((e) => {
      const profileName = e.profileId ? byId.get(e.profileId) : undefined;
      return profileName ? { ...e, profileName } : { ...e };
    });
}

export function clearAuthAuditLog(): void {
  writeEntries([]);
  notifyAuthAuditLogChanged();
}

export function idpHostFromUrl(urlStr: string): string {
  try {
    return new URL(urlStr).hostname || urlStr;
  } catch {
    return urlStr.slice(0, 80);
  }
}

export function maskUsername(username: string): string {
  if (!username?.trim()) return '(empty)';
  const u = username.trim();
  if (u.length <= 1) return '*';
  return `${u[0]}***`;
}

function formatProfileRef(profileId: string | undefined, profileName?: string): string {
  if (!profileId) return '—';
  if (profileName) return `${profileName} (${profileId})`;
  return profileId;
}

/** Human-readable log text for the viewer window (newest last for natural read order). */
export function formatAuthLogsForDisplay(): string {
  const byId = new Map<string, string>();
  for (const p of getProfiles()) {
    const d = profileDisplayName(p);
    if (d) byId.set(p.id, d);
  }
  const entries = prune(readEntries()).sort((a, b) => a.t - b.t);
  if (entries.length === 0) return 'No auth audit entries yet.\n\n(IdP requests and failures are recorded when logging is enabled.)';
  const lines: string[] = [];
  for (const e of entries) {
    const ts = new Date(e.t).toISOString();
    const pref = formatProfileRef(e.profileId, e.profileId ? byId.get(e.profileId) : undefined);
    if (e.type === 'idp_request') {
      lines.push(
        `[${ts}] IdP sign-in request  source=${e.source}  profile=${pref}  host=${e.idpHost ?? '—'}  user=${e.usernameHint ?? '—'}`
      );
    } else if (e.type === 'idp_success') {
      const roles = e.roleCount != null ? String(e.roleCount) : '—';
      const ms = e.durationMs != null ? String(e.durationMs) : '—';
      lines.push(
        `[${ts}] IdP sign-in success  source=${e.source}  profile=${pref}  host=${e.idpHost ?? '—'}  roles=${roles}  ${ms}ms`
      );
    } else {
      lines.push(
        `[${ts}] FAILURE  source=${e.source}  profile=${pref}  error=${e.error ?? '—'}`
      );
    }
  }
  return lines.join('\n');
}

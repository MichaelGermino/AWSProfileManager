import { app } from 'electron';
import fs from 'fs';
import path from 'path';

const APP_NAME = 'AWSProfileManager';

function getAppDataPath(): string {
  const base = process.env.APPDATA || path.join(app.getPath('home'), 'AppData', 'Roaming');
  return path.join(base, APP_NAME);
}

function getUiPrefsPath(): string {
  return path.join(getAppDataPath(), 'ui-prefs.json');
}

export interface UiPrefs {
  sidebarCollapsed: boolean;
  refreshPaused: boolean;
  /** True when pause was triggered by consecutive refresh failures (not manual pause). */
  refreshPausedDueToFailures?: boolean;
}

const defaultPrefs: UiPrefs = {
  sidebarCollapsed: false,
  refreshPaused: false,
  refreshPausedDueToFailures: false,
};

function ensureAppDataDir(): void {
  const dir = getAppDataPath();
  if (!fs.existsSync(dir)) {
    fs.mkdirSync(dir, { recursive: true });
  }
}

export function getUiPrefs(): UiPrefs {
  ensureAppDataDir();
  const filePath = getUiPrefsPath();
  if (!fs.existsSync(filePath)) {
    return { ...defaultPrefs };
  }
  try {
    const raw = fs.readFileSync(filePath, 'utf-8');
    const data = JSON.parse(raw) as Partial<UiPrefs>;
    return { ...defaultPrefs, ...data };
  } catch {
    return { ...defaultPrefs };
  }
}

export function saveUiPrefs(prefs: UiPrefs): void {
  ensureAppDataDir();
  fs.writeFileSync(getUiPrefsPath(), JSON.stringify(prefs, null, 2), 'utf-8');
}

export function getSidebarCollapsed(): boolean {
  return getUiPrefs().sidebarCollapsed;
}

export function setSidebarCollapsed(collapsed: boolean): void {
  const prefs = getUiPrefs();
  prefs.sidebarCollapsed = collapsed;
  saveUiPrefs(prefs);
}

export function getRefreshPausedPref(): boolean {
  return getUiPrefs().refreshPaused ?? false;
}

export function setRefreshPausedPref(paused: boolean): void {
  const prefs = getUiPrefs();
  prefs.refreshPaused = paused;
  saveUiPrefs(prefs);
}

export function getRefreshPausedDueToFailuresPref(): boolean {
  return getUiPrefs().refreshPausedDueToFailures ?? false;
}

export function setRefreshPausedDueToFailuresPref(value: boolean): void {
  const prefs = getUiPrefs();
  prefs.refreshPausedDueToFailures = value;
  saveUiPrefs(prefs);
}

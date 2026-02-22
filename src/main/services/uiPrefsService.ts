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
}

const defaultPrefs: UiPrefs = {
  sidebarCollapsed: false,
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

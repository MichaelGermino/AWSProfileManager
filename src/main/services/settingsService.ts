import { app } from 'electron';
import fs from 'fs';
import path from 'path';
import type { Settings } from '../../shared/types';

const APP_NAME = 'AWSProfileManager';

function getAppDataPath(): string {
  const base = process.env.APPDATA || path.join(app.getPath('home'), 'AppData', 'Roaming');
  return path.join(base, APP_NAME);
}

function getSettingsPath(): string {
  return path.join(getAppDataPath(), 'settings.json');
}

const defaultSettings: Settings = {
  defaultSessionDurationHours: 1,
  defaultIdpEntryUrl: '',
  launchAtStartup: false,
  startMinimizedToTray: false,
  accountDisplayNames: {},
  accountDisplayNamesDefault: {},
  openWebUiApiUrl: '',
  openWebUiApiKey: '',
  openWebUiModel: '',
};

function ensureAppDataDir(): void {
  const dir = getAppDataPath();
  if (!fs.existsSync(dir)) {
    fs.mkdirSync(dir, { recursive: true });
  }
}

export function getSettings(): Settings {
  ensureAppDataDir();
  const filePath = getSettingsPath();
  if (!fs.existsSync(filePath)) {
    return { ...defaultSettings };
  }
  try {
    const raw = fs.readFileSync(filePath, 'utf-8');
    const data = JSON.parse(raw) as Partial<Settings> & { defaultAccountDisplayNames?: Record<string, string> };
    const merged = { ...defaultSettings, ...data };
    // Support legacy key name when migrating from old settings.json
    merged.accountDisplayNamesDefault = data.accountDisplayNamesDefault ?? data.defaultAccountDisplayNames ?? {};
    // First-time or missing key: use default section from file. Once user has customized, use saved only.
    if (data.accountDisplayNames === undefined) {
      merged.accountDisplayNames = { ...merged.accountDisplayNamesDefault };
    } else {
      merged.accountDisplayNames = data.accountDisplayNames;
    }
    return merged;
  } catch {
    return { ...defaultSettings };
  }
}

/** Returns the default account display names from settings (for Restore defaults in UI). */
export function getDefaultAccountDisplayNames(): Record<string, string> {
  const settings = getSettings();
  return { ...(settings.accountDisplayNamesDefault ?? {}) };
}

export function saveSettings(settings: Settings): void {
  ensureAppDataDir();
  fs.writeFileSync(getSettingsPath(), JSON.stringify(settings, null, 2), 'utf-8');
}

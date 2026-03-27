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
  terminalShell: 'powershell',
  bashPath: '',
  authLoggingEnabled: true,
};

function ensureAppDataDir(): void {
  const dir = getAppDataPath();
  if (!fs.existsSync(dir)) {
    fs.mkdirSync(dir, { recursive: true });
  }
}

/**
 * Detect Git for Windows (git-scm) bash.exe using standard install paths.
 * Uses process.env.ProgramFiles so it works when the system drive is not C:.
 * Returns the path if found, otherwise null.
 */
export function getDetectedGitBashPath(): string | null {
  if (process.platform !== 'win32') return null;
  const programFiles = process.env.ProgramFiles || 'C:\\Program Files';
  const programFilesX86 = process.env['ProgramFiles(x86)'] || 'C:\\Program Files (x86)';
  const candidates = [
    path.join(programFiles, 'Git', 'bin', 'bash.exe'),
    path.join(programFilesX86, 'Git', 'bin', 'bash.exe'),
  ];
  for (const p of candidates) {
    try {
      if (fs.existsSync(p)) return p;
    } catch {
      // ignore
    }
  }
  return null;
}

export function getSettings(): Settings {
  ensureAppDataDir();
  const filePath = getSettingsPath();
  let merged: Settings;
  if (!fs.existsSync(filePath)) {
    merged = { ...defaultSettings };
  } else {
    try {
      const raw = fs.readFileSync(filePath, 'utf-8');
      const data = JSON.parse(raw) as Partial<Settings> & { defaultAccountDisplayNames?: Record<string, string> };
      merged = { ...defaultSettings, ...data };
      // Support legacy key name when migrating from old settings.json
      merged.accountDisplayNamesDefault = data.accountDisplayNamesDefault ?? data.defaultAccountDisplayNames ?? {};
      // First-time or missing key: use default section from file. Once user has customized, use saved only.
      if (data.accountDisplayNames === undefined) {
        merged.accountDisplayNames = { ...merged.accountDisplayNamesDefault };
      } else {
        merged.accountDisplayNames = data.accountDisplayNames;
      }
    } catch {
      merged = { ...defaultSettings };
    }
  }
  if (process.platform === 'win32' && !(merged.bashPath ?? '').trim()) {
    const detected = getDetectedGitBashPath();
    if (detected) merged.bashPath = detected;
  }
  return merged;
}

/** Returns the default account display names from settings (for Restore defaults in UI). */
export function getDefaultAccountDisplayNames(): Record<string, string> {
  const settings = getSettings();
  return { ...(settings.accountDisplayNamesDefault ?? {}) };
}

export function saveSettings(settings: Settings): void {
  ensureAppDataDir();
  const toWrite = { ...settings };
  if (!(toWrite.bashPath ?? '').trim()) {
    toWrite.terminalShell = 'powershell';
  }
  fs.writeFileSync(getSettingsPath(), JSON.stringify(toWrite, null, 2), 'utf-8');
}

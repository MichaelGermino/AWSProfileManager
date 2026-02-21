import { dialog } from 'electron';
import fs from 'fs';
import type { BrowserWindow, OpenDialogOptions } from 'electron';
import type { Settings, Profile } from '../../shared/types';
import { getSettings } from './settingsService';
import { getProfiles, replaceAllProfiles } from './profileStorage';
import { saveSettings } from './settingsService';
import { updateTrayMenu } from '../tray';

const BACKUP_VERSION = 1;

export interface BackupData {
  version: number;
  settings: Settings;
  profiles: Profile[];
}

export type BackupResult = { canceled: true } | { success: true; path: string } | { success: false; error: string };
export type RestoreResult =
  | { canceled: true }
  | { success: true }
  | { success: false; error: string };

export async function backupConfig(mainWindow: BrowserWindow | null): Promise<BackupResult> {
  const opts = {
    title: 'Save config backup',
    defaultPath: 'aws-profile-manager-backup.json',
    filters: [{ name: 'JSON', extensions: ['json'] }],
  };
  const result = mainWindow
    ? await dialog.showSaveDialog(mainWindow, opts)
    : await dialog.showSaveDialog(opts);
  if (result.canceled || !result.filePath) return { canceled: true };
  try {
    const settings = getSettings();
    const profiles = getProfiles();
    const data: BackupData = { version: BACKUP_VERSION, settings, profiles };
    fs.writeFileSync(result.filePath, JSON.stringify(data, null, 2), 'utf-8');
    return { success: true, path: result.filePath };
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    return { success: false, error: message };
  }
}

export async function restoreConfig(mainWindow: BrowserWindow | null): Promise<RestoreResult> {
  const openOpts: OpenDialogOptions = {
    title: 'Restore from backup',
    filters: [{ name: 'JSON', extensions: ['json'] }],
    properties: ['openFile'],
  };
  const result = mainWindow
    ? await dialog.showOpenDialog(mainWindow, openOpts)
    : await dialog.showOpenDialog(openOpts);
  if (result.canceled || !result.filePaths?.length) return { canceled: true };
  const filePath = result.filePaths[0];
  try {
    const raw = fs.readFileSync(filePath, 'utf-8');
    const data = JSON.parse(raw) as unknown;
    if (!data || typeof data !== 'object' || !Array.isArray((data as BackupData).profiles)) {
      return { success: false, error: 'Invalid backup file: missing or invalid profiles.' };
    }
    const backup = data as BackupData;
    const settings = backup.settings;
    const profiles = backup.profiles;
    if (!settings || typeof settings !== 'object') {
      return { success: false, error: 'Invalid backup file: missing or invalid settings.' };
    }

    const confirmOpts = {
      type: 'warning' as const,
      title: 'Restore config',
      message: 'Restore from backup?',
      detail: `This will replace your current settings and ${profiles.length} profile(s). Your current data will be overwritten.`,
      buttons: ['Cancel', 'Restore'],
      defaultId: 0,
      cancelId: 0,
    };
    const confirmResult = mainWindow
      ? await dialog.showMessageBox(mainWindow, confirmOpts)
      : await dialog.showMessageBox(confirmOpts);
    if (confirmResult.response !== 1) return { canceled: true };

    saveSettings(settings as Settings);
    replaceAllProfiles(profiles as Profile[]);
    updateTrayMenu();
    return { success: true };
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    return { success: false, error: message };
  }
}

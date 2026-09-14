import { dialog } from 'electron';
import fs from 'fs';
import type { BrowserWindow, OpenDialogOptions } from 'electron';
import type { Settings, Profile, ProfileFolder } from '../../shared/types';
import { getSettings } from './settingsService';
import { getProfiles, replaceAllProfiles } from './profileStorage';
import { getFolders, replaceAllFolders } from './folderStorage';
import { saveSettings } from './settingsService';
import { updateTrayMenu } from '../tray';

/**
 * 2 added `folders`. A version 1 file restores fine — it simply has none, and since its profiles
 * carry no folderId either, the result is a correctly ungrouped list.
 *
 * The reverse is also safe: an older build reading a version 2 file ignores `folders` and writes
 * the profiles back with folderId intact, which this version then renders as ungrouped until the
 * folders are recreated.
 */
const BACKUP_VERSION = 2;

export interface BackupData {
  version: number;
  settings: Settings;
  profiles: Profile[];
  /** Absent in version 1 backups. */
  folders?: ProfileFolder[];
}

export type BackupResult = { canceled: true } | { success: true; path: string } | { success: false; error: string };
export type RestoreResult =
  | { canceled: true }
  | { confirm: true; settings: Settings; profiles: Profile[]; folders: ProfileFolder[] }
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
    const folders = getFolders();
    const data: BackupData = { version: BACKUP_VERSION, settings, profiles, folders };
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

    // A version 1 backup has no folders, and restore is replace-all: keeping the current ones
    // would leave folders that nothing in the restored set belongs to.
    const folders = Array.isArray(backup.folders) ? backup.folders : [];

    return {
      confirm: true,
      settings: settings as Settings,
      profiles: profiles as Profile[],
      folders: folders as ProfileFolder[],
    };
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    return { success: false, error: message };
  }
}

export type ApplyRestoreResult = { success: true } | { success: false; error: string };

export function applyRestore(
  settings: Settings,
  profiles: Profile[],
  folders: ProfileFolder[] = []
): ApplyRestoreResult {
  try {
    saveSettings(settings);
    replaceAllProfiles(profiles);
    // Folders after profiles, matching deleteFolder's ordering: if this throws, the worst case is
    // profiles carrying folderIds with no folders, which renders as ungrouped rather than lost.
    replaceAllFolders(folders);
    updateTrayMenu();
    return { success: true };
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    return { success: false, error: message };
  }
}

import { ipcMain, BrowserWindow, app, shell } from 'electron';
import { getMainWindow, getAppIconDataUrl } from './main';
import { getProfiles, saveProfile, deleteProfile, getProfileById, reorderProfiles } from './services/profileStorage';
import { updateTrayMenu } from './tray';
import { getDashboardState } from './services/dashboardService';
import {
  refreshProfile,
  refreshAllProfiles,
  submitCredentials,
  submitCredentialsForRefreshAll,
  selectRole,
  fetchRolesForIdp,
  fetchRolesWithCredentials,
} from './services/awsAuthService';
import { getCachedRoles } from './services/rolesCache';
import { getSettings, saveSettings, getDefaultAccountDisplayNames } from './services/settingsService';
import {
  openCredentialsFile,
  getCredentialsStatus,
  forgetCredentials,
  getDefaultCredentialsDisplay,
  setDefaultCredentials,
  forgetDefaultCredentials,
} from './services/credentialStorage';
import { getRefreshPaused, setRefreshPaused } from './services/refreshScheduler';
import { getSidebarCollapsed, setSidebarCollapsed } from './services/uiPrefsService';
import { backupConfig, restoreConfig, applyRestore } from './services/configBackup';
import { installUpdateAndRestart, checkForUpdatesNow, getLastUpdateStatus } from './services/autoUpdater';
import { startTerminal, writeToTerminal, resizeTerminal } from './services/ptyService';
import { generateAwsCliExample, getOpenWebUiConfigStatus, fetchOpenWebUiModels } from './services/aiService';
import { getServiceList, getCommandsForService } from './services/awsCliDocsService';
import type { Profile, Settings as SettingsType, AwsRole } from '../shared/types';

export function registerIpcHandlers(mainWindow: BrowserWindow | null): void {
  // Profiles
  ipcMain.handle('profiles:getAll', () => getProfiles());
  ipcMain.handle('profiles:save', (_e, profile: Profile) => {
    saveProfile(profile);
    updateTrayMenu();
  });
  ipcMain.handle('profiles:delete', (_e, id: string) => {
    deleteProfile(id);
    updateTrayMenu();
  });
  ipcMain.handle('profiles:getById', (_e, id: string) => getProfileById(id));
  ipcMain.handle('profiles:reorder', (_e, orderedIds: string[]) => {
    reorderProfiles(orderedIds);
    updateTrayMenu();
  });

  // Dashboard
  ipcMain.handle('dashboard:getState', () => getDashboardState());

  // Auth / Refresh
  ipcMain.handle('auth:refresh', async (_e, profileId: string) => refreshProfile(profileId));
  ipcMain.handle('auth:refreshAll', () => refreshAllProfiles());
  ipcMain.handle('auth:submitCredentials', async (_e, profileId: string, username: string, password: string) =>
    submitCredentials(profileId, username, password)
  );
  ipcMain.handle(
    'auth:submitCredentialsForRefreshAll',
    async (
      _e,
      credentialProfileIds: string[],
      defaultProfileIds: string[],
      username: string,
      password: string
    ) => submitCredentialsForRefreshAll(credentialProfileIds, defaultProfileIds, username, password)
  );
  ipcMain.handle('auth:selectRole', async (_e, profileId: string, roleIndex: number) =>
    selectRole(profileId, roleIndex)
  );
  ipcMain.handle(
    'auth:fetchRoles',
    async (_e, idpEntryUrl: string, useDefaultCredentials: boolean, profileId?: string) =>
      fetchRolesForIdp(idpEntryUrl, { useDefaultCredentials, profileId })
  );
  ipcMain.handle(
    'auth:fetchRolesWithCredentials',
    async (_e, idpEntryUrl: string, username: string, password: string) =>
      fetchRolesWithCredentials(idpEntryUrl, username, password)
  );
  ipcMain.handle('roles:getCached', (_e, idpEntryUrl: string) => getCachedRoles(idpEntryUrl));

  // Settings
  ipcMain.handle('settings:get', () => getSettings());
  ipcMain.handle('settings:save', (_e, settings: SettingsType) => {
    saveSettings(settings);
    try {
      app.setLoginItemSettings({ openAtLogin: settings.launchAtStartup });
    } catch {
      // ignore
    }
  });
  ipcMain.handle('settings:getDefaultAccountDisplayNames', () => getDefaultAccountDisplayNames());
  ipcMain.handle('settings:openCredentialsFile', () => openCredentialsFile());
  ipcMain.handle('app:getVersion', () => app.getVersion());
  ipcMain.handle('app:getIconDataUrl', () => getAppIconDataUrl());
  ipcMain.handle('ui:getSidebarCollapsed', () => getSidebarCollapsed());
  ipcMain.handle('ui:setSidebarCollapsed', (_e, collapsed: boolean) => setSidebarCollapsed(collapsed));
  ipcMain.handle('config:backup', () => backupConfig(mainWindow));
  ipcMain.handle('config:restore', () => restoreConfig(mainWindow));
  ipcMain.handle(
    'config:applyRestore',
    (_e, settings: SettingsType, profiles: Profile[]) => applyRestore(settings, profiles)
  );

  // Credentials (manage saved)
  ipcMain.handle('credentials:getStatus', () => getCredentialsStatus());
  ipcMain.handle('credentials:forget', (_e, profileId: string) => forgetCredentials(profileId));
  ipcMain.handle('credentials:getDefaultDisplay', () => getDefaultCredentialsDisplay());
  ipcMain.handle('credentials:setDefault', (_e, username: string, password: string | null) =>
    setDefaultCredentials(username, password)
  );
  ipcMain.handle('credentials:forgetDefault', () => forgetDefaultCredentials());

  // Scheduler
  ipcMain.handle('scheduler:getPaused', () => getRefreshPaused());
  ipcMain.handle('scheduler:setPaused', (_e, paused: boolean) => setRefreshPaused(paused));

  // DevTools (for debugging)
  ipcMain.handle('openDevTools', () => {
    const win = getMainWindow();
    if (win && !win.isDestroyed()) win.webContents.openDevTools();
  });

  // Updates
  ipcMain.handle('update:installAndRestart', () => installUpdateAndRestart());
  ipcMain.handle('update:checkNow', () => checkForUpdatesNow(() => getMainWindow()));
  ipcMain.handle('update:getStatus', () => getLastUpdateStatus());

  // Window controls (custom title bar on Windows)
  ipcMain.handle('window:minimize', () => getMainWindow()?.minimize());
  ipcMain.handle('window:maximize', () => {
    const w = getMainWindow();
    if (w) w.isMaximized() ? w.unmaximize() : w.maximize();
  });
  ipcMain.handle('window:close', () => getMainWindow()?.hide());
  ipcMain.handle('app:openExternal', (_e: unknown, url: string) => shell.openExternal(url));

  // Terminal (node-pty): use event.sender so each window gets its own PTY
  ipcMain.handle('terminal:start', (e) => {
    startTerminal(e.sender);
  });
  ipcMain.handle('terminal:write', (e, data: string) => {
    writeToTerminal(e.sender, data);
  });
  ipcMain.handle('terminal:resize', (e, cols: number, rows: number) => {
    resizeTerminal(e.sender, cols, rows);
  });

  // AI: generate AWS CLI examples via REST (API key stays in main)
  ipcMain.handle('ai:generate-cli', async (_e, payload: { prompt: string }) => {
    return generateAwsCliExample(payload.prompt);
  });
  ipcMain.handle('ai:getConfigStatus', () => getOpenWebUiConfigStatus());
  ipcMain.handle('ai:getModels', () => fetchOpenWebUiModels());

  // AWS CLI docs (scraped service list + per-service commands, cached on disk)
  ipcMain.handle('awsCli:getServiceList', () => getServiceList());
  ipcMain.handle('awsCli:getCommandsForService', (_e, serviceSlug: string) => getCommandsForService(serviceSlug));
}

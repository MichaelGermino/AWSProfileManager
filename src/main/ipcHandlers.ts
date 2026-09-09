import { ipcMain, BrowserWindow, app, shell, dialog } from 'electron';
import { getMainWindow, getAppIconDataUrl } from './main';
import { getProfiles, saveProfile, deleteProfile, getProfileById, reorderProfiles } from './services/profileStorage';
import { updateTrayMenu } from './tray';
import { getDashboardState } from './services/dashboardService';
import {
  refreshProfile,
  refreshAllProfiles,
  refreshAutoRefreshProfiles,
  submitCredentials,
  submitCredentialsForRefreshAll,
  selectRole,
  fetchRolesForIdp,
  fetchRolesWithCredentials,
} from './services/awsAuthService';
import { getCachedRoles } from './services/rolesCache';
import {
  signIn,
  signOut as ssoSignOut,
  getSessionStatus,
  getAccessToken as getSsoAccessToken,
  listAccountsWithRoles,
  detectRegion,
} from './services/identityCenterService';
import { normalizeStartUrl } from '../shared/ssoOrg';
import { openConsoleForProfile, openMultiSessionOptIn } from './services/consoleSignIn';
import { getPendingChangelog, markChangelogSeen } from './services/changelogService';
import { listInstalledBrowsers } from './services/externalBrowser';
import { exportOrgConfig, importOrgConfig } from './services/orgConfigFile';
import { getSettings, saveSettings, getDefaultAccountDisplayNames } from './services/settingsService';
import {
  openCredentialsFile,
  getCredentialsStatus,
  getDefaultCredentialsDisplay,
  setDefaultCredentials,
  forgetDefaultCredentials,
  getMasterPasswordStatus,
  createMasterPassword,
  unlockWithMasterPassword,
  forgetAllCredentialsAndResetMasterPassword,
} from './services/credentialStorage';
import { clearAuthAuditLog, getAuthAuditEntriesForViewer } from './services/authAuditLog';
import { openAuthLogViewerWindow } from './services/authLogViewer';
import { getRefreshPauseState, setRefreshPaused } from './services/refreshScheduler';
import { getSidebarCollapsed, setSidebarCollapsed } from './services/uiPrefsService';
import { backupConfig, restoreConfig, applyRestore } from './services/configBackup';
import { installUpdateAndRestart, checkForUpdatesNow, getLastUpdateStatus } from './services/autoUpdater';
import { startTerminal, writeToTerminal, resizeTerminal } from './services/ptyService';
import {
  chatWithAi,
  streamChatWithAi,
  getOpenWebUiConfigStatus,
  fetchOpenWebUiModels,
} from './services/aiService';
import type { AiChatMessage } from './services/aiService';

/** In-flight AI streams by request id, so the renderer's Stop button can abort them. */
const activeAiStreams = new Map<string, AbortController>();
import {
  getCachedServiceList,
  parseAndCacheServiceList,
  getCachedCommands,
  parseAndCacheCommands,
} from './services/awsCliDocsService';
import { fetchHtmlWithBrowser } from './services/browserFetchService';
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
  // Direct user action, so an Identity Center profile may open a sign-in window from here.
  ipcMain.handle('auth:refresh', async (_e, profileId: string) =>
    refreshProfile(profileId, undefined, { interactive: true })
  );
  ipcMain.handle('auth:refreshAll', () => refreshAllProfiles());
  ipcMain.handle('auth:refreshAutoRefreshProfiles', () => refreshAutoRefreshProfiles());
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

  // Identity Center (Entra-federated SSO)
  ipcMain.handle('sso:signIn', async (_e, startUrl: string, region: string) =>
    signIn({ startUrl: normalizeStartUrl(startUrl), region })
  );
  ipcMain.handle('sso:getSessionStatus', async (_e, startUrl: string, region: string) =>
    getSessionStatus({ startUrl: normalizeStartUrl(startUrl), region })
  );
  ipcMain.handle('sso:signOut', async (_e, startUrl: string, region: string) =>
    ssoSignOut({ startUrl: normalizeStartUrl(startUrl), region })
  );
  ipcMain.handle('sso:detectRegion', async (_e, startUrl: string) => detectRegion(startUrl));
  ipcMain.handle('sso:listAccounts', async (_e, startUrl: string, region: string) => {
    const org = { startUrl: normalizeStartUrl(startUrl), region };
    try {
      // Interactive: this is only reached from an explicit user action in the profile form
      // or the import wizard, so opening a sign-in window is expected here.
      const accessToken = await getSsoAccessToken(org, { interactive: true });
      if (!accessToken) return { error: 'Sign-in was cancelled.' };
      return { accounts: await listAccountsWithRoles(org, accessToken) };
    } catch (err) {
      return { error: err instanceof Error ? err.message : String(err) };
    }
  });
  // One-click AWS console. The sign-in token stays in main; only success/error crosses IPC.
  // Shareable org configuration (no secrets — see orgConfigFile.ts)
  ipcMain.handle('orgConfig:export', (_e, organizationName?: string) =>
    exportOrgConfig(getMainWindow(), organizationName)
  );
  ipcMain.handle('orgConfig:import', () => importOrgConfig(getMainWindow()));

  // Release notes for the running version, shown once after an update.
  ipcMain.handle('changelog:getPending', () => getPendingChangelog());
  ipcMain.handle('changelog:markSeen', (_e, version: string) => markChangelogSeen(version));

  ipcMain.handle('system:listBrowsers', () => listInstalledBrowsers());
  ipcMain.handle('console:open', async (_e, profileId: string) => openConsoleForProfile(profileId));

  ipcMain.handle('profiles:createMany', (_e, profiles: Profile[]) => {
    for (const profile of profiles) saveProfile(profile);
    updateTrayMenu();
    return { created: profiles.length };
  });

  // Settings
  ipcMain.handle('settings:get', () => getSettings());
  ipcMain.handle('settings:selectBashPath', async () => {
    const w = getMainWindow();
    const opts = {
      title: 'Select Bash executable',
      properties: ['openFile'] as ('openFile')[],
      filters:
        process.platform === 'win32'
          ? [
              { name: 'Executables', extensions: ['exe'] },
              { name: 'All Files', extensions: ['*'] },
            ]
          : [{ name: 'All Files', extensions: ['*'] }],
    };
    const result = w ? await dialog.showOpenDialog(w, opts) : await dialog.showOpenDialog(opts);
    if (result.canceled || !result.filePaths?.length) return { canceled: true };
    return { path: result.filePaths[0] };
  });
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
  ipcMain.handle('logs:openAuthViewer', () => {
    openAuthLogViewerWindow(getMainWindow());
  });
  ipcMain.handle('logs:getAuthAuditEntries', () => getAuthAuditEntriesForViewer());
  ipcMain.handle('logs:clearAuthAudit', () => {
    clearAuthAuditLog();
  });
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
  ipcMain.handle('credentials:getDefaultDisplay', () => getDefaultCredentialsDisplay());
  ipcMain.handle('credentials:setDefault', (_e, username: string, password: string | null) =>
    setDefaultCredentials(username, password)
  );
  ipcMain.handle('credentials:forgetDefault', () => forgetDefaultCredentials());
  // Every handler that can change the lock state rebuilds the tray menu: its items are enabled
  // from isLocked() at build time, so without this the menu keeps whatever state it was built with
  // — still disabled after unlocking, or still live after a reset.
  ipcMain.handle('credentials:getMasterPasswordStatus', async () => {
    // Can clear masterPasswordEnabled when there is nothing encrypted left to unlock.
    const status = await getMasterPasswordStatus();
    updateTrayMenu();
    return status;
  });
  ipcMain.handle(
    'credentials:createMasterPassword',
    async (_e, password: string, confirmPassword: string) => {
      const result = await createMasterPassword(password, confirmPassword);
      updateTrayMenu();
      return result;
    }
  );
  ipcMain.handle('credentials:unlock', async (_e, password: string) => {
    const result = await unlockWithMasterPassword(password);
    updateTrayMenu();
    return result;
  });
  ipcMain.handle('credentials:forgetAllAndResetMasterPassword', async () => {
    // Awaited on purpose: the renderer was previously told the reset was done before it had
    // finished, and the tray rebuild below has to see the post-reset state.
    await forgetAllCredentialsAndResetMasterPassword();
    updateTrayMenu();
    getMainWindow()?.webContents.send('credentials:masterPasswordReset');
  });
  ipcMain.handle('credentials:getMasterPasswordEnabled', () => getSettings().masterPasswordEnabled === true);

  // Re-run the browser multi-session opt-in; see openMultiSessionOptIn for why this is manual.
  ipcMain.handle('console:enableMultiSession', () => openMultiSessionOptIn());

  // Scheduler
  ipcMain.handle('scheduler:getPaused', () => getRefreshPauseState());
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
  ipcMain.handle('terminal:start', (_e, options?: { shell: 'powershell' | 'bash' }) => {
    startTerminal(_e.sender, options);
  });
  ipcMain.handle('terminal:write', (e, data: string) => {
    writeToTerminal(e.sender, data);
  });
  ipcMain.handle('terminal:resize', (e, cols: number, rows: number) => {
    resizeTerminal(e.sender, cols, rows);
  });

  // AI: generate AWS CLI examples via REST (API key stays in main)
  ipcMain.handle('ai:chat', async (_e, payload: { messages: AiChatMessage[] }) => {
    return chatWithAi(payload?.messages ?? []);
  });

  /**
   * Streaming chat. Resolves with the complete reply when the stream ends, while tokens
   * are pushed to the renderer as they arrive on 'ai:chat-chunk'. Keeping the final text
   * on the invoke result means the renderer doesn't have to reassemble it from chunks.
   */
  ipcMain.handle(
    'ai:chat-stream',
    async (e, payload: { requestId: string; messages: AiChatMessage[] }) => {
      const requestId = payload?.requestId;
      if (!requestId) return { content: 'Missing request id.', isError: true };

      const controller = new AbortController();
      activeAiStreams.set(requestId, controller);

      try {
        return await streamChatWithAi(payload?.messages ?? [], controller.signal, (delta) => {
          // The window can be closed mid-stream; sending to a destroyed sender throws.
          if (!e.sender.isDestroyed()) {
            e.sender.send('ai:chat-chunk', { requestId, delta });
          }
        });
      } catch (err) {
        const message = err instanceof Error ? err.message : String(err);
        return { content: `AI request failed: ${message}`, isError: true };
      } finally {
        activeAiStreams.delete(requestId);
      }
    }
  );

  ipcMain.handle('ai:chat-abort', (_e, payload: { requestId: string }) => {
    activeAiStreams.get(payload?.requestId)?.abort();
    return { ok: true };
  });
  ipcMain.handle('ai:getConfigStatus', () => getOpenWebUiConfigStatus());
  ipcMain.handle('ai:getModels', () => fetchOpenWebUiModels());

  // AWS CLI docs (scraped service list + per-service commands, cached on disk)
  ipcMain.handle('awsCli:getCachedServiceList', () => getCachedServiceList());
  ipcMain.handle('awsCli:parseAndCacheServiceList', (_e, html: string) => parseAndCacheServiceList(html));
  ipcMain.handle('awsCli:getCachedCommands', (_e, serviceSlug: string) => getCachedCommands(serviceSlug));
  ipcMain.handle(
    'awsCli:parseAndCacheCommands',
    (_e, serviceSlug: string, html: string) => parseAndCacheCommands(serviceSlug, html)
  );
  ipcMain.handle('awsCli:fetchWithBrowser', (_e, url: string) => fetchHtmlWithBrowser(url));
}

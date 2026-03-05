import { contextBridge, ipcRenderer } from 'electron';

export type UpdateStatus =
  | { type: 'available'; version: string }
  | { type: 'downloading'; percent: number }
  | { type: 'downloaded'; version: string }
  | { type: 'error'; message: string }
  | { type: 'no-update' };

const electronAPI = {
  // Profiles
  getProfiles: () => ipcRenderer.invoke('profiles:getAll'),
  saveProfile: (profile: unknown) => ipcRenderer.invoke('profiles:save', profile),
  deleteProfile: (id: string) => ipcRenderer.invoke('profiles:delete', id),
  getProfileById: (id: string) => ipcRenderer.invoke('profiles:getById', id),
  reorderProfiles: (orderedIds: string[]) => ipcRenderer.invoke('profiles:reorder', orderedIds),

  // Dashboard
  getDashboardState: () => ipcRenderer.invoke('dashboard:getState'),

  // Auth
  refreshProfile: (profileId: string) => ipcRenderer.invoke('auth:refresh', profileId),
  submitCredentials: (profileId: string, username: string, password: string) =>
    ipcRenderer.invoke('auth:submitCredentials', profileId, username, password),
  selectRole: (profileId: string, roleIndex: number) =>
    ipcRenderer.invoke('auth:selectRole', profileId, roleIndex),
  fetchRoles: (idpEntryUrl: string, useDefaultCredentials: boolean, profileId?: string) =>
    ipcRenderer.invoke('auth:fetchRoles', idpEntryUrl, useDefaultCredentials, profileId),
  fetchRolesWithCredentials: (idpEntryUrl: string, username: string, password: string) =>
    ipcRenderer.invoke('auth:fetchRolesWithCredentials', idpEntryUrl, username, password),
  getCachedRoles: (idpEntryUrl: string) => ipcRenderer.invoke('roles:getCached', idpEntryUrl),

  // Settings
  getSettings: () => ipcRenderer.invoke('settings:get'),
  saveSettings: (settings: unknown) => ipcRenderer.invoke('settings:save', settings),
  selectBashPath: () =>
    ipcRenderer.invoke('settings:selectBashPath') as Promise<{ canceled: true } | { path: string }>,
  getDefaultAccountDisplayNames: () => ipcRenderer.invoke('settings:getDefaultAccountDisplayNames'),
  openCredentialsFile: () => ipcRenderer.invoke('settings:openCredentialsFile'),
  getAppVersion: () => ipcRenderer.invoke('app:getVersion') as Promise<string>,
  getAppIconDataUrl: () => ipcRenderer.invoke('app:getIconDataUrl') as Promise<string | null>,
  getSidebarCollapsed: () => ipcRenderer.invoke('ui:getSidebarCollapsed') as Promise<boolean>,
  setSidebarCollapsed: (collapsed: boolean) => ipcRenderer.invoke('ui:setSidebarCollapsed', collapsed),
  /** Synchronous so the custom title bar can render immediately on Windows */
  platform: process.platform,
  openExternal: (url: string) => ipcRenderer.invoke('app:openExternal', url),
  windowMinimize: () => ipcRenderer.invoke('window:minimize'),
  windowMaximize: () => ipcRenderer.invoke('window:maximize'),
  windowClose: () => ipcRenderer.invoke('window:close'),
  backupConfig: () => ipcRenderer.invoke('config:backup'),
  restoreConfig: () => ipcRenderer.invoke('config:restore'),
  applyRestore: (settings: unknown, profiles: unknown) =>
    ipcRenderer.invoke('config:applyRestore', settings, profiles),

  // Credentials
  getCredentialsStatus: () => ipcRenderer.invoke('credentials:getStatus'),
  forgetCredentials: (profileId: string) => ipcRenderer.invoke('credentials:forget', profileId),
  getDefaultCredentialsDisplay: () => ipcRenderer.invoke('credentials:getDefaultDisplay'),
  setDefaultCredentials: (username: string, password: string | null) =>
    ipcRenderer.invoke('credentials:setDefault', username, password),
  forgetDefaultCredentials: () => ipcRenderer.invoke('credentials:forgetDefault'),
  getMasterPasswordStatus: () =>
    ipcRenderer.invoke('credentials:getMasterPasswordStatus') as Promise<
      { needsUnlock: true } | { needsCreateMasterPassword: true } | { unlocked: true }
    >,
  createMasterPassword: (password: string, confirmPassword: string) =>
    ipcRenderer.invoke('credentials:createMasterPassword', password, confirmPassword) as Promise<
      { success: true } | { success: false; error: string }
    >,
  unlockWithMasterPassword: (password: string) =>
    ipcRenderer.invoke('credentials:unlock', password) as Promise<
      { success: true } | { success: false; error: string }
    >,
  forgetAllCredentialsAndResetMasterPassword: () =>
    ipcRenderer.invoke('credentials:forgetAllAndResetMasterPassword'),
  getMasterPasswordEnabled: () => ipcRenderer.invoke('credentials:getMasterPasswordEnabled') as Promise<boolean>,
  onMasterPasswordReset: (cb: () => void) => {
    const handler = () => cb();
    ipcRenderer.on('credentials:masterPasswordReset', handler);
    return () => ipcRenderer.removeListener('credentials:masterPasswordReset', handler);
  },

  // Scheduler
  getRefreshPaused: () => ipcRenderer.invoke('scheduler:getPaused'),
  setRefreshPaused: (paused: boolean) => ipcRenderer.invoke('scheduler:setPaused', paused),
  refreshAutoRefreshProfiles: () => ipcRenderer.invoke('auth:refreshAutoRefreshProfiles'),
  onPausedChanged: (cb: (paused: boolean) => void) => {
    ipcRenderer.on('scheduler:pausedChanged', (_e, paused: boolean) => cb(paused));
  },

  // Debug
  openDevTools: () => ipcRenderer.invoke('openDevTools'),

  // Updates
  installUpdateAndRestart: () => ipcRenderer.invoke('update:installAndRestart'),
  checkForUpdates: () => ipcRenderer.invoke('update:checkNow') as Promise<UpdateStatus>,
  getUpdateStatus: () => ipcRenderer.invoke('update:getStatus') as Promise<UpdateStatus | null>,
  onUpdateStatus: (cb: (status: UpdateStatus) => void) => {
    ipcRenderer.on('update', (_e, status: UpdateStatus) => cb(status));
  },

  // Events from main
  onCredentialsRequired: (cb: (profileId: string, prefillUsername?: string) => void) => {
    ipcRenderer.on('auth:credentialsRequired', (_e, profileId: string, prefillUsername?: string) =>
      cb(profileId, prefillUsername)
    );
  },
  onRefreshAllRequired: (cb: (credentialProfileIds: string[], defaultProfileIds: string[]) => void) => {
    ipcRenderer.on(
      'auth:refreshAllRequired',
      (_e, credentialProfileIds: string[], defaultProfileIds: string[]) =>
        cb(credentialProfileIds, defaultProfileIds)
    );
  },
  submitCredentialsForRefreshAll: (
    credentialProfileIds: string[],
    defaultProfileIds: string[],
    username: string,
    password: string
  ) =>
    ipcRenderer.invoke(
      'auth:submitCredentialsForRefreshAll',
      credentialProfileIds,
      defaultProfileIds,
      username,
      password
    ),
  onCredentialsRefreshed: (cb: (profileId: string) => void) => {
    ipcRenderer.on('auth:credentialsRefreshed', (_e, profileId: string) => cb(profileId));
  },
  onRefreshStarted: (cb: (profileId: string) => void) => {
    ipcRenderer.on('auth:refreshStarted', (_e, profileId: string) => cb(profileId));
  },
  onCredentialsExpired: (cb: (profileId: string, message: string) => void) => {
    ipcRenderer.on('auth:credentialsExpired', (_e, profileId: string, message: string) => cb(profileId, message));
  },
  onNotify: (cb: (title: string, body: string) => void) => {
    ipcRenderer.on('notify', (_e, title: string, body: string) => cb(title, body));
  },

  // Terminal (node-pty in main; data streamed via IPC)
  terminalStart: (options?: { shell: 'powershell' | 'bash' }) =>
    ipcRenderer.invoke('terminal:start', options),
  terminalWrite: (data: string) => ipcRenderer.invoke('terminal:write', data),
  terminalResize: (cols: number, rows: number) => ipcRenderer.invoke('terminal:resize', cols, rows),
  onTerminalData: (cb: (data: string) => void) => {
    const handler = (_e: Electron.IpcRendererEvent, data: string) => cb(data);
    ipcRenderer.on('terminal:data', handler);
    return () => ipcRenderer.removeListener('terminal:data', handler);
  },
  onTerminalError: (cb: (message: string) => void) => {
    const handler = (_e: Electron.IpcRendererEvent, message: string) => cb(message);
    ipcRenderer.on('terminal:error', handler);
    return () => ipcRenderer.removeListener('terminal:error', handler);
  },

  // AI: generate AWS CLI examples (REST called from main; key never exposed)
  generateAwsCli: (payload: { prompt: string }) =>
    ipcRenderer.invoke('ai:generate-cli', payload) as Promise<{ command: string; explanation: string }>,
  getAiConfigStatus: () =>
    ipcRenderer.invoke('ai:getConfigStatus') as Promise<{ configured: boolean }>,
  getAiModels: () =>
    ipcRenderer.invoke('ai:getModels') as Promise<{ models: string[] } | { error: string }>,

  // AWS CLI docs: fetch via hidden BrowserWindow in main (Chromium stack / system certs).
  getAwsCliServiceList: async (): Promise<string[]> => {
    const cached = await ipcRenderer.invoke('awsCli:getCachedServiceList') as string[] | null;
    if (cached != null && Array.isArray(cached)) return cached;
    const INDEX_URL = 'https://docs.aws.amazon.com/cli/latest/';
    const html = await ipcRenderer.invoke('awsCli:fetchWithBrowser', INDEX_URL) as string;
    return ipcRenderer.invoke('awsCli:parseAndCacheServiceList', html) as Promise<string[]>;
  },
  getAwsCliCommandsForService: async (serviceSlug: string): Promise<
    Array<{
      id: string;
      name: string;
      description: string;
      syntax: string;
      options: unknown[];
      examples: unknown[];
      mocked: false;
      docUrl: string;
    }>
  > => {
    const cached = await ipcRenderer.invoke('awsCli:getCachedCommands', serviceSlug) as
      Array<{ id: string; name: string; description: string; syntax: string; options: unknown[]; examples: unknown[]; mocked: false; docUrl: string }> | null;
    if (cached != null && Array.isArray(cached)) return cached;
    const BASE_REFERENCE = 'https://docs.aws.amazon.com/cli/latest/reference';
    const slug = serviceSlug.replace(/\/$/, '').replace(/\.html$/, '').trim();
    const url = `${BASE_REFERENCE}/${slug}/`;
    const html = await ipcRenderer.invoke('awsCli:fetchWithBrowser', url) as string;
    return ipcRenderer.invoke('awsCli:parseAndCacheCommands', serviceSlug, html) as Promise<
      Array<{
        id: string;
        name: string;
        description: string;
        syntax: string;
        options: unknown[];
        examples: unknown[];
        mocked: false;
        docUrl: string;
      }>
    >;
  },
};

contextBridge.exposeInMainWorld('electron', electronAPI);

export type ElectronAPI = typeof electronAPI;

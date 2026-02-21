import { contextBridge, ipcRenderer } from 'electron';

export type UpdateStatus =
  | { type: 'available'; version: string }
  | { type: 'downloading'; percent: number }
  | { type: 'downloaded'; version: string }
  | { type: 'error'; message: string };

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
  getDefaultAccountDisplayNames: () => ipcRenderer.invoke('settings:getDefaultAccountDisplayNames'),
  openCredentialsFile: () => ipcRenderer.invoke('settings:openCredentialsFile'),
  getAppVersion: () => ipcRenderer.invoke('app:getVersion') as Promise<string>,
  backupConfig: () => ipcRenderer.invoke('config:backup'),
  restoreConfig: () => ipcRenderer.invoke('config:restore'),

  // Credentials
  getCredentialsStatus: () => ipcRenderer.invoke('credentials:getStatus'),
  forgetCredentials: (profileId: string) => ipcRenderer.invoke('credentials:forget', profileId),
  getDefaultCredentialsDisplay: () => ipcRenderer.invoke('credentials:getDefaultDisplay'),
  setDefaultCredentials: (username: string, password: string | null) =>
    ipcRenderer.invoke('credentials:setDefault', username, password),
  forgetDefaultCredentials: () => ipcRenderer.invoke('credentials:forgetDefault'),

  // Scheduler
  getRefreshPaused: () => ipcRenderer.invoke('scheduler:getPaused'),
  setRefreshPaused: (paused: boolean) => ipcRenderer.invoke('scheduler:setPaused', paused),
  onPausedChanged: (cb: (paused: boolean) => void) => {
    ipcRenderer.on('scheduler:pausedChanged', (_e, paused: boolean) => cb(paused));
  },

  // Debug
  openDevTools: () => ipcRenderer.invoke('openDevTools'),

  // Updates
  installUpdateAndRestart: () => ipcRenderer.invoke('update:installAndRestart'),
  onUpdateStatus: (cb: (status: UpdateStatus) => void) => {
    ipcRenderer.on('update', (_e, status: UpdateStatus) => cb(status));
  },

  // Events from main
  onCredentialsRequired: (cb: (profileId: string, prefillUsername?: string) => void) => {
    ipcRenderer.on('auth:credentialsRequired', (_e, profileId: string, prefillUsername?: string) =>
      cb(profileId, prefillUsername)
    );
  },
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
};

contextBridge.exposeInMainWorld('electron', electronAPI);

export type ElectronAPI = typeof electronAPI;

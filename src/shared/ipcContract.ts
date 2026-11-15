/**
 * The complete IPC surface between the renderer and the backend, in one place.
 *
 * Today the same channel names are duplicated across src/main/ipcHandlers.ts, src/preload/index.ts
 * and the renderer call sites. This table is the inventory for the Tauri 2.0 rewrite
 * (docs/native-rewrite-plan.md, sections 3 and 5.2):
 *   - the parity test suite must cover every entry,
 *   - the Rust scenario runner calls apm_core::dispatch with the same channel names,
 *   - scripts/check-ipc-contract.mjs fails if main/preload and this file drift apart.
 *
 * `method` is the window.electron (or window.logViewer) property the renderer calls. `null` means the
 * channel is handled in main but has no preload method (kept for parity, see notes).
 * `args` are the METHOD's parameters (what the renderer and the scenario files pass). Where the
 * preload reshapes them for the channel, `notes` says how.
 * Pure data, no Node built-ins: the renderer may import it.
 */

export type IpcWindow = 'main' | 'logViewer';

export interface IpcRequest {
  channel: string;
  method: string | null;
  args: string[];
  /** Short description of the resolved value. */
  returns: string;
  window?: IpcWindow;
  notes?: string;
}

export interface IpcEvent {
  channel: string;
  /** Preload subscription method, or null when nothing in the renderer listens. */
  method: string | null;
  payload: string[];
  window?: IpcWindow;
  notes?: string;
}

export const IPC_REQUESTS: readonly IpcRequest[] = [
  // Profiles
  { channel: 'profiles:getAll', method: 'getProfiles', args: [], returns: 'Profile[]' },
  { channel: 'profiles:save', method: 'saveProfile', args: ['profile: Profile'], returns: 'void', notes: 'rebuilds tray menu' },
  { channel: 'profiles:delete', method: 'deleteProfile', args: ['id: string'], returns: 'void', notes: 'rebuilds tray menu' },
  { channel: 'profiles:getById', method: 'getProfileById', args: ['id: string'], returns: 'Profile | undefined' },
  { channel: 'profiles:applyLayout', method: 'applyProfileLayout', args: ['orderedIds: string[]', 'folderByProfileId: Record<string, string | null>'], returns: 'void', notes: 'order + folder membership in ONE write; ids only' },
  { channel: 'profiles:createMany', method: 'createProfiles', args: ['profiles: Profile[]'], returns: '{ created: number }' },

  // Folders
  { channel: 'folders:getAll', method: 'getFolders', args: [], returns: 'ProfileFolder[]' },
  { channel: 'folders:save', method: 'saveFolder', args: ['folder: Partial<ProfileFolder>'], returns: 'ProfileFolder' },
  { channel: 'folders:delete', method: 'deleteFolder', args: ['id: string'], returns: 'void', notes: 'profiles become ungrouped, never deleted' },
  { channel: 'folders:reorder', method: 'reorderFolders', args: ['orderedIds: string[]'], returns: 'void' },

  // Dashboard
  { channel: 'dashboard:getState', method: 'getDashboardState', args: [], returns: 'DashboardState' },

  // Auth
  { channel: 'auth:refresh', method: 'refreshProfile', args: ['profileId: string'], returns: 'RefreshResult', notes: 'interactive: may open SSO sign-in' },
  { channel: 'auth:refreshAll', method: null, args: [], returns: 'RefreshAllResult', notes: 'no preload method; tray uses the service directly' },
  { channel: 'auth:refreshAutoRefreshProfiles', method: 'refreshAutoRefreshProfiles', args: [], returns: 'RefreshAllResult' },
  { channel: 'auth:submitCredentials', method: 'submitCredentials', args: ['profileId: string', 'username: string', 'password: string'], returns: 'RefreshResult' },
  { channel: 'auth:submitCredentialsForRefreshAll', method: 'submitCredentialsForRefreshAll', args: ['credentialProfileIds: string[]', 'defaultProfileIds: string[]', 'username: string', 'password: string'], returns: 'RefreshAllResult' },
  { channel: 'auth:selectRole', method: 'selectRole', args: ['profileId: string', 'roleIndex: number'], returns: 'RefreshResult' },
  { channel: 'auth:fetchRoles', method: 'fetchRoles', args: ['idpEntryUrl: string', 'useDefaultCredentials: boolean', 'profileId?: string'], returns: 'FetchRolesResult' },
  { channel: 'auth:fetchRolesWithCredentials', method: 'fetchRolesWithCredentials', args: ['idpEntryUrl: string', 'username: string', 'password: string'], returns: 'FetchRolesResult' },
  { channel: 'roles:getCached', method: 'getCachedRoles', args: ['idpEntryUrl: string'], returns: 'AwsRole[] | null' },

  // Identity Center
  { channel: 'sso:signIn', method: 'ssoSignIn', args: ['startUrl: string', 'region: string'], returns: '{ success: true; identity? } | { success: false; error }' },
  { channel: 'sso:getSessionStatus', method: 'ssoGetSessionStatus', args: ['startUrl: string', 'region: string'], returns: '{ signedIn; expiresAt?; identity? }' },
  { channel: 'sso:signOut', method: 'ssoSignOut', args: ['startUrl: string', 'region: string'], returns: 'void' },
  { channel: 'sso:detectRegion', method: 'ssoDetectRegion', args: ['startUrl: string'], returns: '{ region } | { error }' },
  { channel: 'sso:listAccounts', method: 'ssoListAccounts', args: ['startUrl: string', 'region: string'], returns: '{ accounts: SsoAccount[] } | { error }', notes: 'interactive' },

  // Org config, changelog, tour, console
  { channel: 'orgConfig:export', method: 'exportOrgConfig', args: ['organizationName?: string'], returns: '{ canceled } | { success: true; path } | { success: false; error }', notes: 'native save dialog' },
  { channel: 'orgConfig:import', method: 'importOrgConfig', args: [], returns: '{ canceled } | { success: true; config } | { success: false; error }', notes: 'native open dialog' },
  { channel: 'changelog:getPending', method: 'getPendingChangelog', args: [], returns: '{ version; notes; url } | null' },
  { channel: 'changelog:markSeen', method: 'markChangelogSeen', args: ['version: string'], returns: 'void' },
  { channel: 'tour:getState', method: 'getTourState', args: [], returns: '{ seenTourIds; forced; freshInstall; version }' },
  { channel: 'tour:markSeen', method: 'markTourSeen', args: ['id: string'], returns: 'void' },
  { channel: 'tour:resetWhatsNew', method: 'resetWhatsNewState', args: [], returns: 'void' },
  { channel: 'system:listBrowsers', method: 'listBrowsers', args: [], returns: '{ key; name }[]' },
  { channel: 'console:open', method: 'openAwsConsole', args: ['profileId: string'], returns: '{ success: true } | { success: false; error }' },
  { channel: 'console:enableMultiSession', method: 'enableConsoleMultiSession', args: [], returns: '{ success: true } | { success: false; error }' },

  // Settings and app
  { channel: 'settings:get', method: 'getSettings', args: [], returns: 'Settings' },
  { channel: 'settings:save', method: 'saveSettings', args: ['settings: Settings'], returns: 'void', notes: 'applies launch-at-startup; icon change if appIcon/trayIconStyle/trayStatusBadge changed' },
  { channel: 'settings:selectBashPath', method: 'selectBashPath', args: [], returns: '{ canceled } | { path }', notes: 'native open dialog' },
  { channel: 'settings:getDefaultAccountDisplayNames', method: 'getDefaultAccountDisplayNames', args: [], returns: 'Record<string, string>' },
  { channel: 'settings:openCredentialsFile', method: 'openCredentialsFile', args: [], returns: 'void' },
  { channel: 'app:getVersion', method: 'getAppVersion', args: [], returns: 'string' },
  { channel: 'app:getIconDataUrl', method: 'getAppIconDataUrl', args: [], returns: 'string | null' },
  { channel: 'app:listIcons', method: 'listAppIcons', args: [], returns: 'AppIconOption[]' },
  { channel: 'app:openExternal', method: 'openExternal', args: ['url: string'], returns: 'void' },
  { channel: 'logs:openAuthViewer', method: 'openAuthLogViewer', args: [], returns: 'void', notes: 'opens second window' },
  { channel: 'logs:getAuthAuditEntries', method: 'getEntries', args: [], returns: 'AuthAuditEntry[]', window: 'logViewer' },
  { channel: 'logs:clearAuthAudit', method: 'clear', args: [], returns: 'void', window: 'logViewer' },
  { channel: 'ui:getSidebarCollapsed', method: 'getSidebarCollapsed', args: [], returns: 'boolean' },
  { channel: 'ui:setSidebarCollapsed', method: 'setSidebarCollapsed', args: ['collapsed: boolean'], returns: 'void' },
  { channel: 'ui:getCollapsedFolders', method: 'getCollapsedFolders', args: [], returns: 'string[]' },
  { channel: 'ui:setCollapsedFolders', method: 'setCollapsedFolders', args: ['ids: string[]'], returns: 'void' },
  { channel: 'appearance:chooseVideo', method: 'chooseBackgroundVideo', args: [], returns: '{ canceled } | { success: true; fileName; updatedAt } | { success: false; error }', notes: 'native open dialog' },
  { channel: 'appearance:clearVideo', method: 'clearBackgroundVideo', args: [], returns: 'void' },
  { channel: 'config:backup', method: 'backupConfig', args: [], returns: 'BackupResult', notes: 'native save dialog' },
  { channel: 'config:restore', method: 'restoreConfig', args: [], returns: 'RestorePreview', notes: 'native open dialog' },
  { channel: 'config:applyRestore', method: 'applyRestore', args: ['settings: Settings', 'profiles: Profile[]', 'folders: ProfileFolder[]'], returns: 'ApplyRestoreResult', notes: 'reapplies app icon' },

  // Credentials
  { channel: 'credentials:getStatus', method: 'getCredentialsStatus', args: [], returns: 'CredentialsStatus' },
  { channel: 'credentials:getDefaultDisplay', method: 'getDefaultCredentialsDisplay', args: [], returns: 'DefaultCredentialsDisplay' },
  { channel: 'credentials:setDefault', method: 'setDefaultCredentials', args: ['username: string', 'password: string | null'], returns: 'void' },
  { channel: 'credentials:forgetDefault', method: 'forgetDefaultCredentials', args: [], returns: 'void' },
  { channel: 'credentials:getMasterPasswordStatus', method: 'getMasterPasswordStatus', args: [], returns: '{ needsUnlock } | { needsCreateMasterPassword } | { unlocked }', notes: 'rebuilds tray menu' },
  { channel: 'credentials:createMasterPassword', method: 'createMasterPassword', args: ['password: string', 'confirmPassword: string'], returns: '{ success: true } | { success: false; error }', notes: 'rebuilds tray menu' },
  { channel: 'credentials:unlock', method: 'unlockWithMasterPassword', args: ['password: string'], returns: '{ success: true } | { success: false; error }', notes: 'rebuilds tray menu' },
  { channel: 'credentials:forgetAllAndResetMasterPassword', method: 'forgetAllCredentialsAndResetMasterPassword', args: [], returns: 'void', notes: 'awaited; then tray rebuild; then emits credentials:masterPasswordReset' },
  { channel: 'credentials:getMasterPasswordEnabled', method: 'getMasterPasswordEnabled', args: [], returns: 'boolean' },

  // Scheduler
  { channel: 'scheduler:getPaused', method: 'getRefreshPaused', args: [], returns: '{ paused; pausedDueToFailures }' },
  { channel: 'scheduler:setPaused', method: 'setRefreshPaused', args: ['paused: boolean'], returns: 'void' },

  // Debug
  { channel: 'openDevTools', method: 'openDevTools', args: [], returns: 'void' },

  // Updates
  { channel: 'update:installAndRestart', method: 'installUpdateAndRestart', args: [], returns: 'void' },
  { channel: 'update:checkNow', method: 'checkForUpdates', args: [], returns: 'UpdateStatus' },
  { channel: 'update:getStatus', method: 'getUpdateStatus', args: [], returns: 'UpdateStatus | null' },

  // Window
  { channel: 'window:minimize', method: 'windowMinimize', args: [], returns: 'void' },
  { channel: 'window:maximize', method: 'windowMaximize', args: [], returns: 'void', notes: 'toggles maximize/unmaximize' },
  { channel: 'window:close', method: 'windowClose', args: [], returns: 'void', notes: 'hides to tray (2.0: destroys window, process stays)' },

  // Terminal (one PTY per window)
  { channel: 'terminal:start', method: 'terminalStart', args: ["options?: { shell: 'powershell' | 'bash' }"], returns: 'void' },
  { channel: 'terminal:write', method: 'terminalWrite', args: ['data: string'], returns: 'void' },
  { channel: 'terminal:resize', method: 'terminalResize', args: ['cols: number', 'rows: number'], returns: 'void' },

  // AI
  { channel: 'ai:chat', method: 'aiChat', args: ['payload: { messages }'], returns: '{ content; isError? }' },
  { channel: 'ai:chat-stream', method: 'aiChatStream', args: ['payload: { requestId; messages }'], returns: '{ content; isError?; aborted? }', notes: 'chunks on ai:chat-chunk' },
  { channel: 'ai:chat-abort', method: 'aiChatAbort', args: ['requestId: string'], returns: '{ ok: boolean }', notes: 'preload wraps it as { requestId } on the channel' },
  { channel: 'ai:getConfigStatus', method: 'getAiConfigStatus', args: [], returns: '{ configured: boolean }' },
  { channel: 'ai:getModels', method: 'getAiModels', args: [], returns: '{ models: string[] } | { error }' },

  // AWS CLI docs (preload composes these into getAwsCliServiceList / getAwsCliCommandsForService)
  { channel: 'awsCli:getCachedServiceList', method: 'getAwsCliServiceList', args: [], returns: 'string[] | null' },
  { channel: 'awsCli:parseAndCacheServiceList', method: 'getAwsCliServiceList', args: ['html: string'], returns: 'string[]' },
  { channel: 'awsCli:getCachedCommands', method: 'getAwsCliCommandsForService', args: ['serviceSlug: string'], returns: 'AwsCliCommand[] | null' },
  { channel: 'awsCli:parseAndCacheCommands', method: 'getAwsCliCommandsForService', args: ['serviceSlug: string', 'html: string'], returns: 'AwsCliCommand[]' },
  { channel: 'awsCli:fetchWithBrowser', method: 'getAwsCliServiceList', args: ['url: string'], returns: 'string (html)', notes: 'used by both composed methods; 2.0 fetches in Rust' },
];

export const IPC_EVENTS: readonly IpcEvent[] = [
  { channel: 'auth:credentialsRequired', method: 'onCredentialsRequired', payload: ['profileId: string', 'prefillUsername: string'] },
  { channel: 'auth:credentialsRefreshed', method: 'onCredentialsRefreshed', payload: ['profileId: string'] },
  { channel: 'auth:refreshStarted', method: 'onRefreshStarted', payload: ['profileId: string'] },
  { channel: 'auth:credentialsExpired', method: 'onCredentialsExpired', payload: ['profileId: string', 'message: string'] },
  { channel: 'auth:networkUnavailable', method: 'onNetworkUnavailable', payload: ['profileId: string'] },
  { channel: 'auth:refreshAllRequired', method: 'onRefreshAllRequired', payload: ['credentialProfileIds: string[]', 'defaultProfileIds: string[]'] },
  { channel: 'auth:ssoLoginRequired', method: 'onSsoLoginRequired', payload: ['profileId: string', 'startUrl: string', 'region: string'] },
  { channel: 'auth:autoRefreshPausedForFailures', method: 'onAutoRefreshPausedForFailures', payload: [] },
  { channel: 'scheduler:pausedChanged', method: 'onPausedChanged', payload: ['state: { paused; pausedDueToFailures }'] },
  { channel: 'sso:loginProgress', method: 'onSsoLoginProgress', payload: ['progress: SsoLoginProgress'] },
  { channel: 'credentials:masterPasswordReset', method: 'onMasterPasswordReset', payload: [] },
  { channel: 'app:iconChanged', method: 'onAppIconChanged', payload: ['dataUrl: string | null'] },
  { channel: 'notify', method: 'onNotify', payload: ['title: string', 'body: string'] },
  { channel: 'update', method: 'onUpdateStatus', payload: ['status: UpdateStatus'] },
  { channel: 'terminal:data', method: 'onTerminalData', payload: ['data: string'] },
  { channel: 'terminal:error', method: 'onTerminalError', payload: ['message: string'] },
  { channel: 'ai:chat-chunk', method: 'onAiChatChunk', payload: ['payload: { requestId; delta }'] },
  { channel: 'logs:authAuditUpdated', method: 'onUpdated', payload: [], window: 'logViewer' },
];

/**
 * Not IPC, but part of the surface the port must reproduce: main drives these pages with
 * webContents.executeJavaScript, and the renderer reads one synchronous value.
 */
export const NON_IPC_SURFACE = [
  'window.electron.platform (synchronous process.platform)',
  'resources/splash.html (static splash, 5 s floor)',
  'resources/tray-hud.html window.__setHudState(state, title, subtitle) via executeJavaScript',
  'resources/auth-log-viewer.html (window.logViewer bridge)',
  'appmedia:// protocol (background video, Range/206)',
] as const;

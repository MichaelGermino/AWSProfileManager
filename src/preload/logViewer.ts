import { contextBridge, ipcRenderer } from 'electron';

const UPDATED = 'logs:authAuditUpdated';

contextBridge.exposeInMainWorld('logViewer', {
  getEntries: () => ipcRenderer.invoke('logs:getAuthAuditEntries') as Promise<
    Array<{
      t: number;
      type: 'idp_request' | 'idp_success' | 'failure';
      source: string;
      profileId?: string;
      /** Friendly name from current profiles (not stored in log file). */
      profileName?: string;
      idpHost?: string;
      usernameHint?: string;
      error?: string;
      roleCount?: number;
      durationMs?: number;
    }>
  >,
  clear: () => ipcRenderer.invoke('logs:clearAuthAudit') as Promise<void>,
  /** Subscribe to log file changes (append/clear). Returns an unsubscribe function. */
  onUpdated: (callback: () => void) => {
    const handler = () => callback();
    ipcRenderer.on(UPDATED, handler);
    return () => ipcRenderer.removeListener(UPDATED, handler);
  },
});

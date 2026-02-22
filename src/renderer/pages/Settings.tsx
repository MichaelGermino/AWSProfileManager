import { useEffect, useState } from 'react';
import type { Settings, Profile } from '../../shared/types';

declare global {
  interface Window {
    electron: {
      getSettings: () => Promise<Settings>;
      saveSettings: (s: Settings) => Promise<void>;
      getDefaultAccountDisplayNames: () => Promise<Record<string, string>>;
      openCredentialsFile: () => Promise<void>;
      getAppVersion: () => Promise<string>;
      getAppIconDataUrl: () => Promise<string | null>;
      getSidebarCollapsed: () => Promise<boolean>;
      setSidebarCollapsed: (collapsed: boolean) => Promise<void>;
      backupConfig: () => Promise<{ canceled?: boolean; success?: boolean; path?: string; error?: string }>;
      restoreConfig: () => Promise<
        | { canceled?: boolean }
        | { confirm: true; settings: Settings; profiles: Profile[] }
        | { success: false; error: string }
      >;
      applyRestore: (settings: Settings, profiles: Profile[]) => Promise<{ success: boolean; error?: string }>;
      getDefaultCredentialsDisplay: () => Promise<{ username: string; hasPassword: boolean } | null>;
      setDefaultCredentials: (username: string, password: string | null) => Promise<void>;
      forgetDefaultCredentials: () => Promise<void>;
      getRefreshPaused: () => Promise<boolean>;
      setRefreshPaused: (paused: boolean) => Promise<void>;
      onPausedChanged: (cb: (paused: boolean) => void) => void;
      openDevTools: () => Promise<void>;
      installUpdateAndRestart: () => Promise<void>;
      onUpdateStatus: (cb: (status: { type: 'available' | 'downloading' | 'downloaded' | 'error' | 'no-update'; version?: string; percent?: number; message?: string }) => void) => void;
      checkForUpdates: () => Promise<{ type: string; version?: string; message?: string }>;
      platform: string;
      openExternal: (url: string) => Promise<void>;
      windowMinimize: () => Promise<void>;
      windowMaximize: () => Promise<void>;
      windowClose: () => Promise<void>;
    };
  }
}

export default function Settings() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [defaultCreds, setDefaultCreds] = useState<{ username: string; hasPassword: boolean } | null>(null);
  const [defaultUsername, setDefaultUsername] = useState('');
  const [defaultPassword, setDefaultPassword] = useState('');
  const [paused, setPaused] = useState(false);
  const [newAccountId, setNewAccountId] = useState('');
  const [newAccountDisplay, setNewAccountDisplay] = useState('');
  const [configBackupMessage, setConfigBackupMessage] = useState<{ type: 'success' | 'error'; text: string } | null>(null);
  const [restoreConfirm, setRestoreConfirm] = useState<{ settings: Settings; profiles: Profile[] } | null>(null);
  const [forgetCredsConfirm, setForgetCredsConfirm] = useState(false);
  const [restoreDefaultsMessage, setRestoreDefaultsMessage] = useState<string | null>(null);
  const [appVersion, setAppVersion] = useState<string>('');
  const [updateCheckMessage, setUpdateCheckMessage] = useState<string | null>(null);
  const [updateStatus, setUpdateStatus] = useState<{
    type: 'available' | 'downloading' | 'downloaded' | 'error' | 'no-update';
    version?: string;
    message?: string;
  } | null>(null);

  useEffect(() => {
    window.electron.getSettings().then(setSettings);
    window.electron.getRefreshPaused().then(setPaused);
    window.electron.getAppVersion().then(setAppVersion);
  }, []);

  useEffect(() => {
    window.electron.onPausedChanged(setPaused);
  }, []);

  useEffect(() => {
    window.electron.onUpdateStatus((status) =>
      setUpdateStatus({
        type: status.type,
        version: status.version,
        message: status.message,
      })
    );
  }, []);

  useEffect(() => {
    window.electron.getDefaultCredentialsDisplay().then((d) => {
      setDefaultCreds(d ?? null);
      setDefaultUsername(d?.username ?? '');
      setDefaultPassword('');
    });
  }, []);

  const saveSettings = async () => {
    if (!settings) return;
    await window.electron.saveSettings(settings);
  };

  const togglePaused = async () => {
    const next = !paused;
    await window.electron.setRefreshPaused(next);
    setPaused(next);
  };

  const saveDefaultCredentials = async () => {
    await window.electron.setDefaultCredentials(defaultUsername, defaultPassword === '' ? null : defaultPassword);
    const d = await window.electron.getDefaultCredentialsDisplay();
    setDefaultCreds(d ?? null);
    setDefaultUsername(d?.username ?? '');
    setDefaultPassword('');
  };

  const forgetDefaultCredentials = async () => {
    await window.electron.forgetDefaultCredentials();
    setDefaultCreds(null);
    setDefaultUsername('');
    setDefaultPassword('');
    setForgetCredsConfirm(false);
  };

  const addAccountMapping = () => {
    const accountId = newAccountId.trim();
    if (!accountId || !settings) return;
    const nextSettings = {
      ...settings,
      accountDisplayNames: { ...(settings.accountDisplayNames ?? {}), [accountId]: newAccountDisplay.trim() },
    };
    setSettings(nextSettings);
    window.electron.saveSettings(nextSettings);
    setNewAccountId('');
    setNewAccountDisplay('');
  };

  const handleBackupConfig = async () => {
    setConfigBackupMessage(null);
    const result = await window.electron.backupConfig();
    if (result.canceled) return;
    if (result.success && result.path) {
      setConfigBackupMessage({ type: 'success', text: `Backup saved to ${result.path}` });
    } else {
      setConfigBackupMessage({ type: 'error', text: (result as { error?: string }).error ?? 'Backup failed' });
    }
  };

  const handleRestoreConfig = async () => {
    setConfigBackupMessage(null);
    const result = await window.electron.restoreConfig();
    if ('canceled' in result && result.canceled) return;
    if ('success' in result && result.success === false) {
      setConfigBackupMessage({ type: 'error', text: result.error ?? 'Restore failed' });
      return;
    }
    if ('confirm' in result && result.confirm) {
      setRestoreConfirm({ settings: result.settings, profiles: result.profiles });
    }
  };

  const handleConfirmRestore = async () => {
    if (!restoreConfirm) return;
    setConfigBackupMessage(null);
    const applyResult = await window.electron.applyRestore(restoreConfirm.settings, restoreConfirm.profiles);
    setRestoreConfirm(null);
    if (applyResult.success) {
      const fresh = await window.electron.getSettings();
      setSettings(fresh);
      setConfigBackupMessage({ type: 'success', text: 'Config restored. Refresh the Profiles page to see restored profiles.' });
    } else {
      setConfigBackupMessage({ type: 'error', text: applyResult.error ?? 'Restore failed' });
    }
  };

  if (!settings) return null;

  return (
    <div className="space-y-10">
      <div>
        <h2 className="text-3xl font-bold text-discord-text tracking-tight">Settings</h2>
        <p className="mt-1 text-sm text-discord-textMuted">Configure defaults, credentials, and app behavior</p>
      </div>

      <section className="rounded-card bg-discord-panel border border-discord-border overflow-hidden shadow-discord-card">
        <div className="border-l-4 border-discord-accent pl-6 pr-6 pt-6 pb-1">
          <h3 className="text-lg font-bold text-discord-text">General</h3>
          <p className="mt-0.5 text-sm text-discord-textMuted">Version, updates, and startup options</p>
        </div>
        <div className="p-6 pt-4 space-y-4">
          {appVersion ? (
            <div>
              <span className="text-sm text-discord-textMuted">Version </span>
              <span className="text-sm font-medium text-discord-text">{appVersion}</span>
            </div>
          ) : null}
          <div className="flex items-center gap-2 flex-wrap">
            {updateStatus?.type === 'downloaded' ? (
              <button
                type="button"
                onClick={() => window.electron.installUpdateAndRestart()}
                className="rounded-button border border-discord-success/50 bg-discord-darkest px-3 py-1.5 text-sm text-discord-success hover:bg-discord-success/10 transition-colors inline-flex items-center gap-2"
              >
                <svg className="h-4 w-4 flex-shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden>
                  <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M4 16v1a3 3 0 003 3h10a3 3 0 003-3v-1m-4-4l-4 4m0 0l-4-4m4 4V4" />
                </svg>
                Install update
              </button>
            ) : updateStatus?.type === 'error' ? (
              <button
                type="button"
                onClick={async () => {
                  setUpdateCheckMessage(null);
                  try {
                    await window.electron.checkForUpdates();
                  } catch {
                    setUpdateCheckMessage('Update check failed.');
                  }
                }}
                className="rounded-button border border-discord-border bg-discord-darkest px-3 py-1.5 text-sm text-discord-textMuted hover:bg-discord-dark hover:text-discord-text transition-colors inline-flex items-center gap-2"
              >
                <svg className="h-4 w-4 flex-shrink-0 text-discord-danger" fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden>
                  <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z" />
                </svg>
                Retry download
              </button>
            ) : (
              <button
                type="button"
                onClick={async () => {
                  setUpdateCheckMessage('Checking…');
                  try {
                    const result = await window.electron.checkForUpdates();
                    if (result.type === 'no-update') setUpdateCheckMessage('You\'re up to date.');
                    else if (result.type === 'available') setUpdateCheckMessage(`Update available: v${result.version}. Download will start; the button will change when ready.`);
                    else if (result.type === 'error') setUpdateCheckMessage(`Update check failed: ${result.message ?? 'Unknown error'}`);
                    else setUpdateCheckMessage(null);
                  } catch {
                    setUpdateCheckMessage('Update check failed.');
                  }
                }}
                className="rounded-button border border-discord-border bg-discord-darkest px-3 py-1.5 text-sm text-discord-textMuted hover:bg-discord-dark hover:text-discord-text transition-colors"
              >
                Check for updates
              </button>
            )}
            {updateCheckMessage ? (
              <span className="text-sm text-discord-textMuted">{updateCheckMessage}</span>
            ) : null}
          </div>
          <div>
            <label className="block text-sm text-discord-textMuted">Default session duration (hours)</label>
            <input
              type="number"
              min={1}
              max={12}
              value={settings.defaultSessionDurationHours}
              onChange={(e) =>
                setSettings((s) => s ? { ...s, defaultSessionDurationHours: parseInt(e.target.value, 10) || 1 } : s)
              }
              onBlur={saveSettings}
              className="mt-1.5 w-24 rounded-button border border-discord-border bg-discord-darkest px-3 py-2 text-discord-text focus:border-discord-accent focus:outline-none transition-colors"
            />
          </div>
          <div>
            <label className="block text-sm text-discord-textMuted">Default IdP entry URL</label>
            <input
              type="url"
              value={settings.defaultIdpEntryUrl}
              onChange={(e) =>
                setSettings((s) => (s ? { ...s, defaultIdpEntryUrl: e.target.value } : s))
              }
              onBlur={saveSettings}
              className="mt-1.5 w-full rounded-button border border-discord-border bg-discord-darkest px-3 py-2 text-discord-text placeholder-discord-textMuted focus:border-discord-accent focus:outline-none transition-colors"
              placeholder="https://adfs.example.com/adfs/ls/..."
            />
            <p className="mt-1 text-xs text-discord-textMuted">Used when creating a new profile; you can change it per profile.</p>
          </div>
          <label className="flex items-center gap-2">
            <input
              type="checkbox"
              checked={settings.launchAtStartup}
              onChange={(e) => {
                const next = settings ? { ...settings, launchAtStartup: e.target.checked } : null;
                if (next) {
                  setSettings(next);
                  window.electron.saveSettings(next);
                }
              }}
              className="rounded border-discord-border text-discord-accent focus:ring-discord-accent"
            />
            <span className="text-sm text-discord-textMuted">Launch at startup</span>
          </label>
          <label className="flex items-center gap-2">
            <input
              type="checkbox"
              checked={settings.startMinimizedToTray}
              onChange={(e) => {
                const next = settings ? { ...settings, startMinimizedToTray: e.target.checked } : null;
                if (next) {
                  setSettings(next);
                  window.electron.saveSettings(next);
                }
              }}
              className="rounded border-discord-border text-discord-accent focus:ring-discord-accent"
            />
            <span className="text-sm text-discord-textMuted">Start minimized to tray</span>
          </label>
        </div>
      </section>

      <section className="rounded-card bg-discord-panel border border-discord-border overflow-hidden shadow-discord-card">
        <div className="border-l-4 border-discord-accent pl-6 pr-6 pt-6 pb-1">
          <h3 className="text-lg font-bold text-discord-text">Debug</h3>
          <p className="mt-0.5 text-sm text-discord-textMuted">Developer tools and diagnostics</p>
        </div>
        <div className="p-6 pt-4">
        <p className="mb-2 text-sm text-discord-textMuted">
          Open Developer Tools to see console logs and network errors when refreshing.
        </p>
        <button
          onClick={() => window.electron.openDevTools()}
          className="rounded-button border border-discord-border bg-discord-darkest px-4 py-2.5 text-sm text-discord-textMuted hover:bg-discord-dark hover:text-discord-text transition-colors"
        >
          Open Developer Tools
        </button>
        </div>
      </section>
      <section className="rounded-card bg-discord-panel border border-discord-border overflow-hidden shadow-discord-card">
        <div className="border-l-4 border-discord-accent pl-6 pr-6 pt-6 pb-1">
          <h3 className="text-lg font-bold text-discord-text">AWS credentials file</h3>
          <p className="mt-0.5 text-sm text-discord-textMuted">Open the credentials file in your editor</p>
        </div>
        <div className="p-6 pt-4">
        <button
          onClick={() => window.electron.openCredentialsFile()}
          className="rounded-button bg-discord-accent px-5 py-2.5 text-sm font-semibold text-white shadow-discord-accent hover:bg-discord-accentHover hover:shadow-discord-accent-hover transition-all duration-200"
        >
          Open credentials file
        </button>
        </div>
      </section>

      <section className="rounded-card bg-discord-panel border border-discord-border overflow-hidden shadow-discord-card">
        <div className="border-l-4 border-discord-accent pl-6 pr-6 pt-6 pb-1">
          <h3 className="text-lg font-bold text-discord-text">Backup &amp; restore</h3>
          <p className="mt-0.5 text-sm text-discord-textMuted">Save or restore your config and profiles</p>
        </div>
        <div className="p-6 pt-4">
        <p className="mb-4 text-sm text-discord-textMuted">
          Back up or restore your settings and profiles. You choose the file location. Restore will replace your current config after you confirm.
        </p>
        <div className="flex flex-wrap gap-2">
          <button
            type="button"
            onClick={handleBackupConfig}
            className="rounded-button bg-discord-accent px-5 py-2.5 text-sm font-semibold text-white shadow-discord-accent hover:bg-discord-accentHover hover:shadow-discord-accent-hover transition-all duration-200"
          >
            Back up config
          </button>
          <button
            type="button"
            onClick={handleRestoreConfig}
            className="rounded-button border border-discord-border bg-discord-darkest px-4 py-2.5 text-sm text-discord-textMuted hover:bg-discord-dark hover:text-discord-text transition-colors"
          >
            Restore config
          </button>
        </div>
        {configBackupMessage && (
          <p className={`mt-3 text-sm font-medium ${configBackupMessage.type === 'success' ? 'text-discord-success' : 'text-discord-danger'}`}>
            {configBackupMessage.text}
          </p>
        )}
        </div>
      </section>

      <section className="rounded-card bg-discord-panel border border-discord-border overflow-hidden shadow-discord-card">
        <div className="border-l-4 border-discord-accent pl-6 pr-6 pt-6 pb-1">
          <h3 className="text-lg font-bold text-discord-text">Auto refresh</h3>
          <p className="mt-0.5 text-sm text-discord-textMuted">Pause or resume automatic credential refresh</p>
        </div>
        <div className="p-6 pt-4">
        <button
          onClick={togglePaused}
          className={`rounded-button px-5 py-2.5 text-sm font-semibold transition-colors ${paused ? 'bg-discord-success text-white hover:opacity-90' : 'border border-discord-border bg-discord-darkest text-discord-textMuted hover:bg-discord-dark hover:text-discord-text'}`}
        >
          {paused ? 'Resume auto refresh' : 'Pause auto refresh'}
        </button>
        </div>
      </section>

      <section className="rounded-card bg-discord-panel border border-discord-border overflow-hidden shadow-discord-card">
        <div className="border-l-4 border-discord-accent pl-6 pr-6 pt-6 pb-1">
          <h3 className="text-lg font-bold text-discord-text">Default credentials</h3>
          <p className="mt-0.5 text-sm text-discord-textMuted">Optional credentials used when refreshing profiles</p>
        </div>
        <div className="p-6 pt-4">
        <p className="mb-4 text-sm text-discord-textMuted">
          Optional. Profiles can be set to use these credentials when refreshing (no prompt). You can save username only
          or username and password. Leave password blank when saving to keep the existing password.
        </p>
        <div className="mb-4 space-y-3">
          <div>
            <label className="block text-sm text-discord-textMuted">Username</label>
            <input
              type="text"
              value={defaultUsername}
              onChange={(e) => setDefaultUsername(e.target.value)}
              className="mt-1.5 w-full max-w-xs rounded-button border border-discord-border bg-discord-darkest px-3 py-2 text-discord-text placeholder-discord-textMuted focus:border-discord-accent focus:outline-none transition-colors"
              placeholder="e.g. you@company.com"
            />
          </div>
          <div>
            <label className="block text-sm text-discord-textMuted">Password (optional)</label>
            <input
              type="password"
              value={defaultPassword}
              onChange={(e) => setDefaultPassword(e.target.value)}
              className="mt-1.5 w-full max-w-xs rounded-button border border-discord-border bg-discord-darkest px-3 py-2 text-discord-text placeholder-discord-textMuted focus:border-discord-accent focus:outline-none transition-colors"
              placeholder="Leave blank to keep existing"
            />
          </div>
          <div className="flex gap-2">
            <button
              onClick={saveDefaultCredentials}
              className="rounded-button bg-discord-accent px-5 py-2.5 text-sm font-semibold text-white shadow-discord-accent hover:bg-discord-accentHover hover:shadow-discord-accent-hover transition-all duration-200"
            >
              Save default credentials
            </button>
            {defaultCreds && (defaultCreds.username || defaultCreds.hasPassword) && (
              <button
                onClick={() => setForgetCredsConfirm(true)}
                className="rounded-button border border-discord-danger/50 bg-discord-danger/20 px-4 py-2 text-sm text-discord-danger hover:bg-discord-danger/30 transition-colors"
              >
                Forget default
              </button>
            )}
          </div>
        </div>
        </div>
      </section>

      <section className="rounded-card bg-discord-panel border border-discord-border overflow-hidden shadow-discord-card">
        <div className="border-l-4 border-discord-accent pl-6 pr-6 pt-6 pb-1">
          <h3 className="text-lg font-bold text-discord-text">Account display names</h3>
          <p className="mt-0.5 text-sm text-discord-textMuted">Map AWS account IDs to friendly names in the role picker</p>
        </div>
        <div className="p-6 pt-4">
        <p className="mb-4 text-sm text-discord-textMuted">
          Map AWS account IDs to friendly names shown in the role dropdown. The app only receives the SAML form, not the IdP role-picker page, so add mappings here to get friendly labels. If you need to undo changes, use Restore defaults to reset from the list stored in settings.json (accountDisplayNamesDefault).
        </p>
        <div className="space-y-2">
          {Object.entries(settings.accountDisplayNames ?? {}).map(([accountId, displayName]) => (
            <div key={accountId} className="flex items-center gap-2">
              <input
                type="text"
                value={accountId}
                readOnly
                className="w-36 rounded-button border border-discord-border bg-discord-darkest/50 px-2 py-1.5 text-sm text-discord-textMuted"
              />
              <span className="text-discord-textMuted">→</span>
              <input
                type="text"
                value={displayName}
                onChange={(e) =>
                  setSettings((s) =>
                    s
                      ? {
                          ...s,
                          accountDisplayNames: { ...(s.accountDisplayNames ?? {}), [accountId]: e.target.value },
                        }
                      : s
                  )
                }
                onBlur={saveSettings}
                className="flex-1 max-w-xs rounded-button border border-discord-border bg-discord-darkest px-2 py-1.5 text-sm text-discord-text focus:border-discord-accent focus:outline-none"
                placeholder="Display name"
              />
              <button
                type="button"
                onClick={() => {
                  const next = { ...(settings.accountDisplayNames ?? {}) };
                  delete next[accountId];
                  const nextSettings = { ...settings, accountDisplayNames: next };
                  setSettings(nextSettings);
                  window.electron.saveSettings(nextSettings);
                }}
                className="rounded-button px-2 py-1 text-sm text-discord-danger hover:bg-discord-danger/20 transition-colors"
              >
                Remove
              </button>
            </div>
          ))}
          <div className="flex items-center gap-2 pt-2">
            <input
              type="text"
              placeholder="Account ID"
              value={newAccountId}
              onChange={(e) => setNewAccountId(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && addAccountMapping()}
              className="w-44 rounded-button border border-discord-border bg-discord-darkest px-2 py-1.5 text-sm text-discord-text placeholder-discord-textMuted focus:border-discord-accent focus:outline-none"
            />
            <input
              type="text"
              placeholder="Display name"
              value={newAccountDisplay}
              onChange={(e) => setNewAccountDisplay(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && addAccountMapping()}
              className="flex-1 max-w-xs rounded-button border border-discord-border bg-discord-darkest px-2 py-1.5 text-sm text-discord-text placeholder-discord-textMuted focus:border-discord-accent focus:outline-none"
            />
            <button
              type="button"
              onClick={addAccountMapping}
              className="rounded-button border border-discord-border bg-discord-darkest px-3 py-1.5 text-sm text-discord-textMuted hover:bg-discord-dark hover:text-discord-text transition-colors"
            >
              Add
            </button>
          </div>
        </div>
        <p className="mt-2 text-xs text-discord-textMuted">
          Enter Account ID and Display name, then click Add or press Enter.
        </p>
        <div className="mt-4 flex gap-2">
          <button
            type="button"
            onClick={async () => {
              setRestoreDefaultsMessage(null);
              const defaults = await window.electron.getDefaultAccountDisplayNames();
              if (Object.keys(defaults).length === 0) {
                setRestoreDefaultsMessage('There are no names to restore.');
                return;
              }
              if (!settings) return;
              const nextSettings = { ...settings, accountDisplayNames: { ...defaults } };
              setSettings(nextSettings);
              await window.electron.saveSettings(nextSettings);
            }}
            className="rounded-button border border-discord-border bg-discord-darkest px-3 py-1.5 text-sm text-discord-textMuted hover:bg-discord-dark hover:text-discord-text transition-colors"
          >
            Restore defaults
          </button>
          {restoreDefaultsMessage != null && (
            <span className="text-sm text-discord-textMuted">{restoreDefaultsMessage}</span>
          )}
        </div>
        </div>
      </section>

      {restoreConfirm && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center modal-backdrop p-4"
          onClick={() => setRestoreConfirm(null)}
        >
          <div
            className="w-full max-w-md rounded-card bg-discord-panel border border-discord-border p-6 shadow-discord-modal animate-modal-in"
            onClick={(e) => e.stopPropagation()}
          >
            <h3 className="text-lg font-semibold text-discord-text">Restore config</h3>
            <p className="mt-2 text-sm text-discord-textMuted">
              Restore from backup? This will replace your current settings and {restoreConfirm.profiles.length} profile(s). Your current data will be overwritten.
            </p>
            <div className="mt-6 flex justify-end gap-2">
              <button
                onClick={() => setRestoreConfirm(null)}
                className="rounded-button border border-discord-border bg-discord-darkest px-4 py-2 text-sm text-discord-textMuted hover:bg-discord-dark hover:text-discord-text transition-colors"
              >
                Cancel
              </button>
              <button
                onClick={handleConfirmRestore}
                className="rounded-button bg-discord-accent px-4 py-2 text-sm font-medium text-white hover:bg-discord-accentHover transition-colors"
              >
                Restore
              </button>
            </div>
          </div>
        </div>
      )}

      {forgetCredsConfirm && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center modal-backdrop p-4"
          onClick={() => setForgetCredsConfirm(false)}
        >
          <div
            className="w-full max-w-md rounded-card bg-discord-panel border border-discord-border p-6 shadow-discord-modal animate-modal-in"
            onClick={(e) => e.stopPropagation()}
          >
            <h3 className="text-lg font-semibold text-discord-text">Remove default credentials?</h3>
            <p className="mt-2 text-sm text-discord-textMuted">
              You will be prompted for username and password for each profile that uses default credentials.
            </p>
            <div className="mt-6 flex justify-end gap-2">
              <button
                onClick={() => setForgetCredsConfirm(false)}
                className="rounded-button border border-discord-border bg-discord-darkest px-4 py-2 text-sm text-discord-textMuted hover:bg-discord-dark hover:text-discord-text transition-colors"
              >
                Cancel
              </button>
              <button
                onClick={forgetDefaultCredentials}
                className="rounded-button bg-discord-danger px-4 py-2 text-sm font-medium text-white hover:opacity-90 transition-opacity"
              >
                Forget default
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

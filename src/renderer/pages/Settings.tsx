import { useEffect, useState } from 'react';
import type { Settings } from '../../shared/types';

declare global {
  interface Window {
    electron: {
      getSettings: () => Promise<Settings>;
      saveSettings: (s: Settings) => Promise<void>;
      getDefaultAccountDisplayNames: () => Promise<Record<string, string>>;
      openCredentialsFile: () => Promise<void>;
      getAppVersion: () => Promise<string>;
      backupConfig: () => Promise<{ canceled?: boolean; success?: boolean; path?: string; error?: string }>;
      restoreConfig: () => Promise<{ canceled?: boolean; success?: boolean; error?: string }>;
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
  const [newDefaultAccountId, setNewDefaultAccountId] = useState('');
  const [newDefaultAccountDisplay, setNewDefaultAccountDisplay] = useState('');
  const [configBackupMessage, setConfigBackupMessage] = useState<{ type: 'success' | 'error'; text: string } | null>(null);
  const [appVersion, setAppVersion] = useState<string>('');
  const [updateCheckMessage, setUpdateCheckMessage] = useState<string | null>(null);

  useEffect(() => {
    window.electron.getSettings().then(setSettings);
    window.electron.getRefreshPaused().then(setPaused);
    window.electron.getAppVersion().then(setAppVersion);
  }, []);

  useEffect(() => {
    window.electron.onPausedChanged(setPaused);
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
    if (!confirm('Remove default credentials? You will be prompted for each profile that uses them.')) return;
    await window.electron.forgetDefaultCredentials();
    setDefaultCreds(null);
    setDefaultUsername('');
    setDefaultPassword('');
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
    if (result.canceled) return;
    if (result.success) {
      const fresh = await window.electron.getSettings();
      setSettings(fresh);
      setConfigBackupMessage({ type: 'success', text: 'Config restored. Refresh the Profiles page to see restored profiles.' });
    } else {
      setConfigBackupMessage({ type: 'error', text: (result as { error?: string }).error ?? 'Restore failed' });
    }
  };

  const addDefaultAccountMapping = () => {
    const accountId = newDefaultAccountId.trim();
    if (!accountId || !settings) return;
    const nextSettings = {
      ...settings,
      defaultAccountDisplayNames: { ...(settings.defaultAccountDisplayNames ?? {}), [accountId]: newDefaultAccountDisplay.trim() },
    };
    setSettings(nextSettings);
    window.electron.saveSettings(nextSettings);
    setNewDefaultAccountId('');
    setNewDefaultAccountDisplay('');
  };

  if (!settings) return null;

  return (
    <div className="space-y-8">
      <h2 className="text-2xl font-semibold text-discord-text">Settings</h2>

      <section className="rounded-lg bg-discord-panel p-6 shadow">
        <h3 className="mb-4 text-lg font-medium text-discord-text">General</h3>
        <div className="space-y-4">
          {appVersion ? (
            <div>
              <span className="text-sm text-discord-textMuted">Version </span>
              <span className="text-sm font-medium text-discord-text">{appVersion}</span>
            </div>
          ) : null}
          <div className="flex items-center gap-2">
            <button
              type="button"
              onClick={async () => {
                setUpdateCheckMessage('Checking…');
                try {
                  const result = await window.electron.checkForUpdates();
                  if (result.type === 'no-update') setUpdateCheckMessage('You\'re up to date.');
                  else if (result.type === 'available') setUpdateCheckMessage(`Update available: v${result.version}. It will appear at the top of the app.`);
                  else if (result.type === 'error') setUpdateCheckMessage(`Update check failed: ${result.message ?? 'Unknown error'}`);
                  else setUpdateCheckMessage(null);
                } catch {
                  setUpdateCheckMessage('Update check failed.');
                }
              }}
              className="rounded bg-discord-darkest px-3 py-1.5 text-sm text-discord-textMuted hover:bg-discord-dark"
            >
              Check for updates
            </button>
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
              className="mt-1 w-24 rounded border border-discord-darkest bg-discord-darkest px-3 py-2 text-discord-text"
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
              className="mt-1 w-full rounded border border-discord-darkest bg-discord-darkest px-3 py-2 text-discord-text placeholder-discord-textMuted focus:border-discord-accent focus:outline-none"
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
              className="rounded border-discord-darkest"
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
              className="rounded border-discord-darkest"
            />
            <span className="text-sm text-discord-textMuted">Start minimized to tray</span>
          </label>
        </div>
      </section>

      <section className="rounded-lg bg-discord-panel p-6 shadow">
        <h3 className="mb-4 text-lg font-medium text-discord-text">Debug</h3>
        <p className="mb-2 text-sm text-discord-textMuted">
          Open Developer Tools to see console logs and network errors when refreshing.
        </p>
        <button
          onClick={() => window.electron.openDevTools()}
          className="mb-6 rounded bg-discord-darkest px-4 py-2 text-sm text-discord-textMuted hover:bg-discord-dark"
        >
          Open Developer Tools
        </button>
      </section>
      <section className="rounded-lg bg-discord-panel p-6 shadow">
        <h3 className="mb-4 text-lg font-medium text-discord-text">AWS credentials file</h3>
        <button
          onClick={() => window.electron.openCredentialsFile()}
          className="rounded bg-discord-accent px-4 py-2 text-sm text-white hover:bg-discord-accentHover"
        >
          Open credentials file
        </button>
      </section>

      <section className="rounded-lg bg-discord-panel p-6 shadow">
        <h3 className="mb-4 text-lg font-medium text-discord-text">Backup &amp; restore</h3>
        <p className="mb-4 text-sm text-discord-textMuted">
          Back up or restore your settings and profiles. You choose the file location. Restore will replace your current config after you confirm.
        </p>
        <div className="flex flex-wrap gap-2">
          <button
            type="button"
            onClick={handleBackupConfig}
            className="rounded bg-discord-accent px-4 py-2 text-sm text-white hover:bg-discord-accentHover"
          >
            Back up config
          </button>
          <button
            type="button"
            onClick={handleRestoreConfig}
            className="rounded bg-discord-darkest px-4 py-2 text-sm text-discord-textMuted hover:bg-discord-dark"
          >
            Restore config
          </button>
        </div>
        {configBackupMessage && (
          <p className={`mt-3 text-sm ${configBackupMessage.type === 'success' ? 'text-discord-success' : 'text-discord-danger'}`}>
            {configBackupMessage.text}
          </p>
        )}
      </section>

      <section className="rounded-lg bg-discord-panel p-6 shadow">
        <h3 className="mb-4 text-lg font-medium text-discord-text">Auto refresh</h3>
        <button
          onClick={togglePaused}
          className={`rounded px-4 py-2 text-sm ${paused ? 'bg-discord-success text-white' : 'bg-discord-darkest text-discord-textMuted'}`}
        >
          {paused ? 'Resume auto refresh' : 'Pause auto refresh'}
        </button>
      </section>

      <section className="rounded-lg bg-discord-panel p-6 shadow">
        <h3 className="mb-4 text-lg font-medium text-discord-text">Default credentials</h3>
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
              className="mt-1 w-full max-w-xs rounded border border-discord-darkest bg-discord-darkest px-3 py-2 text-discord-text placeholder-discord-textMuted focus:border-discord-accent focus:outline-none"
              placeholder="e.g. you@company.com"
            />
          </div>
          <div>
            <label className="block text-sm text-discord-textMuted">Password (optional)</label>
            <input
              type="password"
              value={defaultPassword}
              onChange={(e) => setDefaultPassword(e.target.value)}
              className="mt-1 w-full max-w-xs rounded border border-discord-darkest bg-discord-darkest px-3 py-2 text-discord-text placeholder-discord-textMuted focus:border-discord-accent focus:outline-none"
              placeholder="Leave blank to keep existing"
            />
          </div>
          <div className="flex gap-2">
            <button
              onClick={saveDefaultCredentials}
              className="rounded bg-discord-accent px-4 py-2 text-sm text-white hover:bg-discord-accentHover"
            >
              Save default credentials
            </button>
            {defaultCreds && (defaultCreds.username || defaultCreds.hasPassword) && (
              <button
                onClick={forgetDefaultCredentials}
                className="rounded bg-discord-danger/20 px-4 py-2 text-sm text-discord-danger hover:bg-discord-danger/30"
              >
                Forget default
              </button>
            )}
          </div>
        </div>
      </section>

      <section className="rounded-lg bg-discord-panel p-6 shadow">
        <h3 className="mb-4 text-lg font-medium text-discord-text">Default account display names</h3>
        <p className="mb-4 text-sm text-discord-textMuted">
          Template stored in settings.json. Add your account ID → display name mappings here. Click &quot;Restore defaults&quot; in the section below to copy this list into the current display names.
        </p>
        <div className="space-y-2">
          {Object.entries(settings.defaultAccountDisplayNames ?? {}).map(([accountId, displayName]) => (
            <div key={accountId} className="flex items-center gap-2">
              <input
                type="text"
                value={accountId}
                readOnly
                className="w-36 rounded border border-discord-darkest bg-discord-darkest/50 px-2 py-1.5 text-sm text-discord-textMuted"
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
                          defaultAccountDisplayNames: { ...(s.defaultAccountDisplayNames ?? {}), [accountId]: e.target.value },
                        }
                      : s
                  )
                }
                onBlur={saveSettings}
                className="flex-1 max-w-xs rounded border border-discord-darkest bg-discord-darkest px-2 py-1.5 text-sm text-discord-text"
                placeholder="Display name"
              />
              <button
                type="button"
                onClick={() => {
                  const next = { ...(settings.defaultAccountDisplayNames ?? {}) };
                  delete next[accountId];
                  const nextSettings = { ...settings, defaultAccountDisplayNames: next };
                  setSettings(nextSettings);
                  window.electron.saveSettings(nextSettings);
                }}
                className="rounded px-2 py-1 text-sm text-discord-danger hover:bg-discord-danger/20"
              >
                Remove
              </button>
            </div>
          ))}
          <div className="flex items-center gap-2 pt-2">
            <input
              type="text"
              placeholder="Account ID"
              value={newDefaultAccountId}
              onChange={(e) => setNewDefaultAccountId(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && addDefaultAccountMapping()}
              className="w-44 rounded border border-discord-darkest bg-discord-darkest px-2 py-1.5 text-sm text-discord-text placeholder-discord-textMuted"
            />
            <input
              type="text"
              placeholder="Display name"
              value={newDefaultAccountDisplay}
              onChange={(e) => setNewDefaultAccountDisplay(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && addDefaultAccountMapping()}
              className="flex-1 max-w-xs rounded border border-discord-darkest bg-discord-darkest px-2 py-1.5 text-sm text-discord-text placeholder-discord-textMuted"
            />
            <button
              type="button"
              onClick={addDefaultAccountMapping}
              className="rounded bg-discord-darkest px-3 py-1.5 text-sm text-discord-textMuted hover:bg-discord-dark"
            >
              Add
            </button>
          </div>
        </div>
      </section>

      <section className="rounded-lg bg-discord-panel p-6 shadow">
        <h3 className="mb-4 text-lg font-medium text-discord-text">Account display names (current)</h3>
        <p className="mb-4 text-sm text-discord-textMuted">
          Map AWS account IDs to friendly names shown in the role dropdown. The app only receives the SAML form, not the IdP role-picker page, so add mappings here to get friendly labels. Restore defaults copies from the default list above.
        </p>
        <div className="space-y-2">
          {Object.entries(settings.accountDisplayNames ?? {}).map(([accountId, displayName]) => (
            <div key={accountId} className="flex items-center gap-2">
              <input
                type="text"
                value={accountId}
                readOnly
                className="w-36 rounded border border-discord-darkest bg-discord-darkest/50 px-2 py-1.5 text-sm text-discord-textMuted"
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
                className="flex-1 max-w-xs rounded border border-discord-darkest bg-discord-darkest px-2 py-1.5 text-sm text-discord-text"
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
                className="rounded px-2 py-1 text-sm text-discord-danger hover:bg-discord-danger/20"
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
              className="w-44 rounded border border-discord-darkest bg-discord-darkest px-2 py-1.5 text-sm text-discord-text placeholder-discord-textMuted"
            />
            <input
              type="text"
              placeholder="Display name"
              value={newAccountDisplay}
              onChange={(e) => setNewAccountDisplay(e.target.value)}
              onKeyDown={(e) => e.key === 'Enter' && addAccountMapping()}
              className="flex-1 max-w-xs rounded border border-discord-darkest bg-discord-darkest px-2 py-1.5 text-sm text-discord-text placeholder-discord-textMuted"
            />
            <button
              type="button"
              onClick={addAccountMapping}
              className="rounded bg-discord-darkest px-3 py-1.5 text-sm text-discord-textMuted hover:bg-discord-dark"
            >
              Add
            </button>
          </div>
        </div>
        <p className="mt-2 text-xs text-discord-textMuted">
          Enter Account ID and Display name, then click Add or press Enter.
        </p>
        <div className="mt-3 flex gap-2">
          <button
            type="button"
            onClick={async () => {
              const defaults = await window.electron.getDefaultAccountDisplayNames();
              if (!settings) return;
              const nextSettings = { ...settings, accountDisplayNames: defaults };
              setSettings(nextSettings);
              await window.electron.saveSettings(nextSettings);
            }}
            className="rounded bg-discord-darkest px-3 py-1.5 text-sm text-discord-textMuted hover:bg-discord-dark"
          >
            Restore defaults
          </button>
        </div>
      </section>
    </div>
  );
}

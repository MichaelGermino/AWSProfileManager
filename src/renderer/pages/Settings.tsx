import { useEffect, useState, useRef, useCallback } from 'react';
import { createPortal } from 'react-dom';
import type { Settings, Profile } from '../../shared/types';
import { validateMasterPassword } from '../../shared/masterPassword';
import { CreateMasterPasswordModal } from '../components/CreateMasterPasswordModal';
import { Tooltip } from '../components/Tooltip';

declare global {
  interface Window {
    electron: {
      getSettings: () => Promise<Settings>;
      saveSettings: (s: Settings) => Promise<void>;
      selectBashPath: () => Promise<{ canceled: true } | { path: string }>;
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
      getDefaultCredentialsDisplay: () => Promise<{ username: string; hasPassword: boolean; locked?: boolean } | null>;
      setDefaultCredentials: (username: string, password: string | null) => Promise<void>;
      forgetDefaultCredentials: () => Promise<void>;
      getRefreshPaused: () => Promise<boolean>;
      setRefreshPaused: (paused: boolean) => Promise<void>;
      onPausedChanged: (cb: (paused: boolean) => void) => void;
      openDevTools: () => Promise<void>;
      installUpdateAndRestart: () => Promise<void>;
      onUpdateStatus: (cb: (status: { type: 'available' | 'downloading' | 'downloaded' | 'error' | 'no-update'; version?: string; percent?: number; message?: string }) => void) => void;
      checkForUpdates: () => Promise<{ type: string; version?: string; message?: string }>;
      getAiModels: () => Promise<{ models: string[] } | { error: string }>;
      platform: string;
      openExternal: (url: string) => Promise<void>;
      windowMinimize: () => Promise<void>;
      windowMaximize: () => Promise<void>;
      windowClose: () => Promise<void>;
    };
  }
}

const IconRefresh = ({ className = 'w-4 h-4' }: { className?: string }) => (
  <svg className={className} fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden>
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M4 4v5h.582m15.356 2A8.001 8.001 0 004.582 9m0 0H9m11 11v-5h-.581m0 0a8.003 8.003 0 01-15.357-2m15.357 2H15" />
  </svg>
);
export default function Settings() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [defaultCreds, setDefaultCreds] = useState<{ username: string; hasPassword: boolean; locked?: boolean } | null>(null);
  const [masterPasswordEnabled, setMasterPasswordEnabled] = useState(false);
  const [forgetAllConfirm, setForgetAllConfirm] = useState(false);
  const [showCreateMasterPasswordForSave, setShowCreateMasterPasswordForSave] = useState(false);
  const [createMasterPasswordAcknowledged, setCreateMasterPasswordAcknowledged] = useState(false);
  const [createMasterPasswordHoldProgress, setCreateMasterPasswordHoldProgress] = useState(0);
  const createMasterPasswordHoldRafRef = useRef<number | null>(null);
  const createMasterPasswordHoldStartRef = useRef<number>(0);
  const [createMasterPasswordValue, setCreateMasterPasswordValue] = useState('');
  const [createMasterPasswordConfirm, setCreateMasterPasswordConfirm] = useState('');
  const [createMasterPasswordError, setCreateMasterPasswordError] = useState('');
  const [createMasterPasswordSubmitting, setCreateMasterPasswordSubmitting] = useState(false);
  const [defaultCredsSaveError, setDefaultCredsSaveError] = useState('');
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
  const [checkingForUpdates, setCheckingForUpdates] = useState(false);
  const [openWebUiModels, setOpenWebUiModels] = useState<string[]>([]);
  const [openWebUiModelsLoading, setOpenWebUiModelsLoading] = useState(false);
  const [openWebUiModelsError, setOpenWebUiModelsError] = useState<string | null>(null);
  const [modelDropdownOpen, setModelDropdownOpen] = useState(false);
  const [modelSearchQuery, setModelSearchQuery] = useState('');
  const [modelDropdownRect, setModelDropdownRect] = useState<DOMRect | null>(null);
  const modelTriggerRef = useRef<HTMLButtonElement>(null);
  const modelsFetchedRef = useRef(false);

  useEffect(() => {
    modelsFetchedRef.current = false;
  }, [settings?.openWebUiApiUrl, settings?.openWebUiApiKey]);

  useEffect(() => {
    if (!modelDropdownOpen) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === 'Escape') setModelDropdownOpen(false);
    };
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [modelDropdownOpen]);

  useEffect(() => {
    if (modelDropdownOpen && modelTriggerRef.current) {
      setModelDropdownRect(modelTriggerRef.current.getBoundingClientRect());
    } else {
      setModelDropdownRect(null);
    }
  }, [modelDropdownOpen]);

  const fetchModels = useCallback(() => {
    setOpenWebUiModelsLoading(true);
    setOpenWebUiModelsError(null);
    window.electron.getAiModels?.().then((result: { models: string[] } | { error: string }) => {
      if (result && 'error' in result) {
        setOpenWebUiModelsError(result.error);
        setOpenWebUiModels([]);
      } else {
        setOpenWebUiModels(result?.models ?? []);
        setOpenWebUiModelsError(null);
      }
      setOpenWebUiModelsLoading(false);
    });
  }, []);

  useEffect(() => {
    window.electron.getSettings().then(setSettings);
    window.electron.getRefreshPaused().then(setPaused);
    window.electron.getAppVersion().then(setAppVersion);
  }, []);

  useEffect(() => {
    window.electron.onPausedChanged(setPaused);
  }, []);

  useEffect(() => {
    window.electron.getUpdateStatus?.()?.then((status) => {
      if (status)
        setUpdateStatus({
          type: status.type,
          version: status.version,
          message: status.message,
        });
    });
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
      setDefaultUsername(d?.locked ? '' : (d?.username ?? ''));
      setDefaultPassword('');
    });
  }, []);

  useEffect(() => {
    (window.electron as { getMasterPasswordEnabled?: () => Promise<boolean> }).getMasterPasswordEnabled?.().then(setMasterPasswordEnabled);
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
    setDefaultCredsSaveError('');
    if (!defaultUsername.trim()) {
      setDefaultCredsSaveError('Username is required.');
      return;
    }
    const result = await window.electron.setDefaultCredentials(
      defaultUsername.trim(),
      defaultPassword === '' ? null : defaultPassword
    ) as void | { success: false; error: string };
    if (result && typeof result === 'object' && result.success === false && result.error === 'MASTER_PASSWORD_REQUIRED') {
      setShowCreateMasterPasswordForSave(true);
      setCreateMasterPasswordValue('');
      setCreateMasterPasswordConfirm('');
      setCreateMasterPasswordError('');
      return;
    }
    const d = await window.electron.getDefaultCredentialsDisplay();
    setDefaultCreds(d ?? null);
    setDefaultUsername(d?.username ?? '');
    setDefaultPassword('');
  };

  const handleCreateMasterPasswordAndSave = async () => {
    setCreateMasterPasswordError('');
    if (!createMasterPasswordValue.trim()) {
      setCreateMasterPasswordError('Please enter a password.');
      return;
    }
    if (createMasterPasswordValue !== createMasterPasswordConfirm) return;
    const requirementError = validateMasterPassword(createMasterPasswordValue);
    if (requirementError) {
      setCreateMasterPasswordError(requirementError);
      return;
    }
    const createMasterPassword = (window.electron as { createMasterPassword?: (p: string, c: string) => Promise<{ success: true } | { success: false; error: string }> }).createMasterPassword;
    if (!createMasterPassword) return;
    setCreateMasterPasswordSubmitting(true);
    const result = await createMasterPassword(createMasterPasswordValue, createMasterPasswordConfirm);
    if (!result.success) {
      setCreateMasterPasswordError(result.error);
      setCreateMasterPasswordSubmitting(false);
      return;
    }
    await window.electron.setDefaultCredentials(defaultUsername, defaultPassword === '' ? null : defaultPassword);
    setShowCreateMasterPasswordForSave(false);
    setCreateMasterPasswordAcknowledged(false);
    setCreateMasterPasswordValue('');
    setCreateMasterPasswordConfirm('');
    setCreateMasterPasswordSubmitting(false);
    setMasterPasswordEnabled(true);
    const d = await window.electron.getDefaultCredentialsDisplay();
    setDefaultCreds(d ?? null);
    setDefaultUsername(d?.username ?? '');
    setDefaultPassword('');
  };

  const clearCreateMasterPasswordHold = () => {
    if (createMasterPasswordHoldRafRef.current != null) {
      cancelAnimationFrame(createMasterPasswordHoldRafRef.current);
      createMasterPasswordHoldRafRef.current = null;
    }
    setCreateMasterPasswordHoldProgress(0);
  };

  const startCreateMasterPasswordHold = () => {
    if (createMasterPasswordHoldRafRef.current != null) return;
    createMasterPasswordHoldStartRef.current = Date.now();
    const tick = () => {
      const elapsed = Date.now() - createMasterPasswordHoldStartRef.current;
      const pct = Math.min(100, (elapsed / 1000) * 100);
      setCreateMasterPasswordHoldProgress(pct);
      if (pct >= 100) {
        if (createMasterPasswordHoldRafRef.current != null) cancelAnimationFrame(createMasterPasswordHoldRafRef.current);
        createMasterPasswordHoldRafRef.current = null;
        setCreateMasterPasswordHoldProgress(0);
        setCreateMasterPasswordAcknowledged(true);
        return;
      }
      createMasterPasswordHoldRafRef.current = requestAnimationFrame(tick);
    };
    createMasterPasswordHoldRafRef.current = requestAnimationFrame(tick);
  };

  useEffect(() => () => { if (createMasterPasswordHoldRafRef.current != null) cancelAnimationFrame(createMasterPasswordHoldRafRef.current); }, []);

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
                Update ready
              </button>
            ) : updateStatus?.type === 'error' ? (
              <button
                type="button"
                onClick={async () => {
                  setUpdateCheckMessage(null);
                  setCheckingForUpdates(true);
                  try {
                    await window.electron.checkForUpdates();
                  } catch {
                    setUpdateCheckMessage('Update check failed.');
                  } finally {
                    setCheckingForUpdates(false);
                  }
                }}
                disabled={checkingForUpdates}
                className="rounded-button border border-discord-border bg-discord-darkest px-3 py-1.5 text-sm text-discord-textMuted hover:bg-discord-dark hover:text-discord-text transition-colors inline-flex items-center gap-2 disabled:opacity-50"
              >
                {checkingForUpdates ? (
                  <svg className="h-4 w-4 flex-shrink-0 animate-spin" fill="none" viewBox="0 0 24 24" aria-hidden>
                    <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
                    <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z" />
                  </svg>
                ) : (
                  <svg className="h-4 w-4 flex-shrink-0 text-discord-danger" fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden>
                    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z" />
                  </svg>
                )}
                Retry download
              </button>
            ) : (
              <button
                type="button"
                onClick={async () => {
                  setUpdateCheckMessage(null);
                  setCheckingForUpdates(true);
                  try {
                    const result = await window.electron.checkForUpdates();
                    if (result.type === 'no-update') setUpdateCheckMessage('You\'re up to date.');
                    else if (result.type === 'available') setUpdateCheckMessage(`Update available: v${result.version ?? ''}`);
                    else if (result.type === 'error') setUpdateCheckMessage(`Update check failed: ${result.message ?? 'Unknown error'}`);
                    else setUpdateCheckMessage(null);
                  } catch {
                    setUpdateCheckMessage('Update check failed.');
                  } finally {
                    setCheckingForUpdates(false);
                  }
                }}
                disabled={checkingForUpdates}
                className="rounded-button border border-discord-border bg-discord-darkest px-3 py-1.5 text-sm text-discord-textMuted hover:bg-discord-dark hover:text-discord-text transition-colors inline-flex items-center gap-2 disabled:opacity-50"
              >
                {checkingForUpdates ? (
                  <svg className="h-4 w-4 flex-shrink-0 animate-spin" fill="none" viewBox="0 0 24 24" aria-hidden>
                    <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
                    <path className="opacity-75" fill="currentColor" d="M4 12a8 8 0 018-8V0C5.373 0 0 5.373 0 12h4zm2 5.291A7.962 7.962 0 014 12H0c0 3.042 1.135 5.824 3 7.938l3-2.647z" />
                  </svg>
                ) : null}
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
          <h3 className="text-lg font-bold text-discord-text">Open WebUI Integration</h3>
          <p className="mt-0.5 text-sm text-discord-textMuted">API URL and key for the Terminal screen AI Assistant (AWS CLI examples)</p>
        </div>
        <div className="p-6 pt-4 space-y-4">
          <div>
            <label htmlFor="openwebui-api-url" className="block text-sm text-discord-textMuted">API URL</label>
            <input
              id="openwebui-api-url"
              type="url"
              value={settings.openWebUiApiUrl ?? ''}
              onChange={(e) => setSettings((s) => (s ? { ...s, openWebUiApiUrl: e.target.value } : s))}
              onBlur={saveSettings}
              className="mt-1.5 w-full max-w-md rounded-button border border-discord-border bg-discord-darkest px-3 py-2 text-discord-text placeholder-discord-textMuted focus:border-discord-accent focus:outline-none transition-colors"
              placeholder="https://your-openwebui-instance.com/api"
            />
          </div>
          <div>
            <label htmlFor="openwebui-api-key" className="block text-sm text-discord-textMuted">API key</label>
            <input
              id="openwebui-api-key"
              type="password"
              value={settings.openWebUiApiKey ?? ''}
              onChange={(e) => setSettings((s) => (s ? { ...s, openWebUiApiKey: e.target.value } : s))}
              onBlur={saveSettings}
              className="mt-1.5 w-full max-w-md rounded-button border border-discord-border bg-discord-darkest px-3 py-2 text-discord-text placeholder-discord-textMuted focus:border-discord-accent focus:outline-none transition-colors"
              placeholder="Your Open WebUI API key"
              autoComplete="off"
            />
          </div>
          <div className="w-full max-w-md">
            {(() => {
              const openWebUiPartiallyConfigured = !!(settings.openWebUiApiUrl?.trim() || settings.openWebUiApiKey?.trim());
              const modelRequired = openWebUiPartiallyConfigured;
              return (
                <>
                  <label className="block text-sm text-discord-textMuted">
                    Model {modelRequired && <span className="text-discord-danger">*</span>}
                  </label>
                  {modelRequired && !settings.openWebUiModel?.trim() && (
                    <p className="mt-1 text-sm text-discord-danger">Please select a model to use Open WebUI.</p>
                  )}
                </>
              );
            })()}
            <div className="mt-1.5 flex gap-2">
              <button
                ref={modelTriggerRef}
                type="button"
                onClick={() => {
                  setModelDropdownOpen((open) => {
                    const next = !open;
                    if (next && !modelsFetchedRef.current) {
                      modelsFetchedRef.current = true;
                      fetchModels();
                    }
                    return next;
                  });
                  setModelSearchQuery('');
                }}
                className="flex-1 flex items-center justify-between gap-2 rounded-button border border-discord-border bg-discord-darkest px-3 py-2 text-left text-discord-text placeholder-discord-textMuted focus:border-discord-accent focus:outline-none transition-colors"
                aria-expanded={modelDropdownOpen}
                aria-haspopup="listbox"
                aria-label="Select Open WebUI model"
              >
                <span className={settings.openWebUiModel ? 'text-discord-text' : 'text-discord-textMuted'}>
                  {settings.openWebUiModel || 'Select model'}
                </span>
                <svg className="h-4 w-4 flex-shrink-0 text-discord-textMuted" fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden>
                  <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M19 9l-7 7-7-7" />
                </svg>
              </button>
              <Tooltip label="Refresh model list" placement="left">
                <button
                  type="button"
                  onClick={() => fetchModels()}
                  disabled={openWebUiModelsLoading || !settings.openWebUiApiUrl?.trim() || !settings.openWebUiApiKey?.trim()}
                  className="inline-flex items-center justify-center rounded-button border border-discord-border bg-discord-darkest p-2 text-discord-textMuted hover:bg-discord-dark hover:text-discord-text transition-colors disabled:opacity-50"
                >
                  <IconRefresh className={`w-5 h-5 ${openWebUiModelsLoading ? 'animate-spin' : ''}`} />
                </button>
              </Tooltip>
            </div>
          </div>
          {modelDropdownOpen &&
            modelDropdownRect &&
            createPortal(
              <>
                <div
                  className="fixed inset-0 z-[100]"
                  aria-hidden
                  onClick={() => setModelDropdownOpen(false)}
                />
                <div
                  className="fixed z-[101] rounded-lg border border-discord-border bg-discord-darker shadow-discord-modal py-1 flex flex-col max-h-64"
                  role="listbox"
                  style={{
                    top: modelDropdownRect.bottom + 4,
                    left: modelDropdownRect.left,
                    width: modelDropdownRect.width,
                  }}
                >
                  <input
                    type="text"
                    value={modelSearchQuery}
                    onChange={(e) => setModelSearchQuery(e.target.value)}
                    placeholder="Search models…"
                    className="mx-2 mb-1 px-2 py-1.5 rounded border border-discord-border bg-discord-darkest text-discord-text text-sm placeholder-discord-textMuted focus:border-discord-accent focus:outline-none"
                    aria-label="Search models"
                    onKeyDown={(e) => e.stopPropagation()}
                  />
                  <div className="overflow-y-auto min-h-0 flex-1">
                    {openWebUiModelsLoading && (
                      <p className="px-3 py-2 text-sm text-discord-textMuted">Loading models…</p>
                    )}
                    {openWebUiModelsError && !openWebUiModelsLoading && (
                      <p className="px-3 py-2 text-sm text-discord-danger">{openWebUiModelsError}</p>
                    )}
                    {!openWebUiModelsLoading && !openWebUiModelsError && (() => {
                      const q = modelSearchQuery.trim().toLowerCase();
                      const filtered = q
                        ? openWebUiModels.filter((id) => id.toLowerCase().includes(q))
                        : openWebUiModels;
                      if (filtered.length === 0) {
                        return <p className="px-3 py-2 text-sm text-discord-textMuted">No models match.</p>;
                      }
                      const openWebUiPartiallyConfigured = !!(settings.openWebUiApiUrl?.trim() || settings.openWebUiApiKey?.trim());
                      return (
                        <ul className="list-none p-0 m-0">
                          {!openWebUiPartiallyConfigured && (
                            <li>
                              <button
                                type="button"
                                onClick={() => {
                                  const next = settings ? { ...settings, openWebUiModel: '' } : settings;
                                  if (next) {
                                    setSettings(next);
                                    window.electron.saveSettings(next);
                                  }
                                  setModelDropdownOpen(false);
                                }}
                                className="w-full text-left px-3 py-2 text-sm text-discord-textMuted hover:bg-discord-panel hover:text-discord-text"
                                role="option"
                              >
                                (None)
                              </button>
                            </li>
                          )}
                          {filtered.map((id) => (
                            <li key={id}>
                              <button
                                type="button"
                                onClick={() => {
                                  const next = settings ? { ...settings, openWebUiModel: id } : settings;
                                  if (next) {
                                    setSettings(next);
                                    window.electron.saveSettings(next);
                                  }
                                  setModelDropdownOpen(false);
                                }}
                                className={`w-full text-left px-3 py-2 text-sm hover:bg-discord-panel ${
                                  settings.openWebUiModel === id ? 'bg-discord-accent/20 text-discord-accent' : 'text-discord-text'
                                }`}
                                role="option"
                                aria-selected={settings.openWebUiModel === id}
                              >
                                {id}
                              </button>
                            </li>
                          ))}
                        </ul>
                      );
                    })()}
                  </div>
                </div>
              </>,
              document.body
            )}
        </div>
      </section>

      <section className="rounded-card bg-discord-panel border border-discord-border overflow-hidden shadow-discord-card">
        <div className="border-l-4 border-discord-accent pl-6 pr-6 pt-6 pb-1">
          <h3 className="text-lg font-bold text-discord-text">Embedded Terminal</h3>
          <p className="mt-0.5 text-sm text-discord-textMuted">
            {window.electron?.platform === 'win32'
              ? "Bash uses Git for Windows (git-scm). Point to Git's bin\\bash.exe."
              : 'Bash path for the Terminal screen when you choose Bash (e.g. /bin/bash).'}
          </p>
        </div>
        <div className="p-6 pt-4 space-y-4">
          <div>
            <label htmlFor="bash-path" className="block text-sm text-discord-textMuted">Bash executable path</label>
            <div className="mt-1.5 flex gap-2 max-w-md">
              <input
                id="bash-path"
                type="text"
                value={settings.bashPath ?? ''}
                onChange={(e) => setSettings((s) => (s ? { ...s, bashPath: e.target.value } : s))}
                onBlur={saveSettings}
                className="flex-1 min-w-0 rounded-button border border-discord-border bg-discord-darkest px-3 py-2 text-discord-text placeholder-discord-textMuted focus:border-discord-accent focus:outline-none transition-colors"
                placeholder={window.electron?.platform === 'win32' ? 'C:\\Program Files\\Git\\bin\\bash.exe' : '/bin/bash'}
                spellCheck={false}
              />
              <button
                type="button"
                onClick={async () => {
                  const result = await window.electron.selectBashPath?.();
                  if (result && !('canceled' in result) && result.path && settings) {
                    const next = { ...settings, bashPath: result.path };
                    setSettings(next);
                    await window.electron.saveSettings(next);
                  }
                }}
                className="flex-shrink-0 rounded-button border border-discord-border bg-discord-darker px-3 py-2 text-sm text-discord-text hover:bg-discord-dark hover:text-discord-text transition-colors"
              >
                Browse…
              </button>
            </div>
            <p className="mt-1 text-xs text-discord-textMuted">
              {window.electron?.platform === 'win32'
                ? 'Use bin\\bash.exe so Bash runs in the app.'
                : 'Required to use Bash on the Terminal screen.'}
            </p>
          </div>
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
          or username and password. Leave password blank when saving to keep the existing password. Credentials are encrypted and stored in the systems keystore/credential manager.
        </p>
        {defaultCreds?.locked && (
          <p className="mb-4 text-sm text-discord-textMuted rounded-button border border-discord-border bg-discord-darkest/50 px-3 py-2">
            Saved credentials are locked. Enter your master password when you start the app to unlock and view or edit.
          </p>
        )}
        <div className="mb-4 space-y-3">
          <div>
            <label className="block text-sm text-discord-textMuted">Username</label>
            <input
              type="text"
              value={defaultUsername}
              onChange={(e) => {
                setDefaultUsername(e.target.value);
                setDefaultCredsSaveError('');
              }}
              disabled={defaultCreds?.locked}
              className="mt-1.5 w-full max-w-xs rounded-button border border-discord-border bg-discord-darkest px-3 py-2 text-discord-text placeholder-discord-textMuted focus:border-discord-accent focus:outline-none transition-colors disabled:opacity-60"
              placeholder={defaultCreds?.locked ? 'Locked' : 'e.g. you@company.com'}
            />
          </div>
          <div>
            <label className="block text-sm text-discord-textMuted">Password (optional)</label>
            <input
              type="password"
              value={defaultPassword}
              onChange={(e) => setDefaultPassword(e.target.value)}
              disabled={defaultCreds?.locked}
              className="mt-1.5 w-full max-w-xs rounded-button border border-discord-border bg-discord-darkest px-3 py-2 text-discord-text placeholder-discord-textMuted focus:border-discord-accent focus:outline-none transition-colors disabled:opacity-60"
              placeholder={defaultCreds?.locked ? 'Locked' : 'Leave blank to keep existing'}
            />
          </div>
          <div className="flex flex-wrap gap-2 items-center">
            <button
              onClick={saveDefaultCredentials}
              disabled={defaultCreds?.locked}
              className="rounded-button bg-discord-accent px-5 py-2.5 text-sm font-semibold text-white shadow-discord-accent hover:bg-discord-accentHover hover:shadow-discord-accent-hover transition-all duration-200 disabled:opacity-50"
            >
              Save default credentials
            </button>
            {defaultCreds && !defaultCreds.locked && (defaultCreds.username || defaultCreds.hasPassword) && (
              <button
                onClick={() => setForgetCredsConfirm(true)}
                className="rounded-button border border-discord-danger/50 bg-discord-danger/20 px-4 py-2 text-sm text-discord-danger hover:bg-discord-danger/30 transition-colors"
              >
                Forget default
              </button>
            )}
            {masterPasswordEnabled && (
              <button
                type="button"
                onClick={() => setForgetAllConfirm(true)}
                className="rounded-button border border-discord-border bg-discord-darkest/50 px-4 py-2 text-sm text-discord-textMuted hover:text-discord-danger hover:border-discord-danger/50 transition-colors"
              >
                Forgot master password?
              </button>
            )}
            {defaultCredsSaveError && (
              <span className="text-sm text-discord-danger">{defaultCredsSaveError}</span>
            )}
          </div>
        </div>
        </div>
      </section>

      <section className="rounded-card bg-discord-panel border border-discord-border overflow-hidden shadow-discord-card">
        <div className="border-l-4 border-discord-accent pl-6 pr-6 pt-6 pb-1">
          <h3 className="text-lg font-bold text-discord-text">Account display names</h3>
          <p className="mt-0.5 text-sm text-discord-textMuted">Map AWS account IDs to friendly names in the role picker when adding or editing a profile</p>
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

      {forgetAllConfirm && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center modal-backdrop p-4"
          onClick={() => setForgetAllConfirm(false)}
        >
          <div
            className="w-full max-w-md rounded-card bg-discord-panel border border-discord-border p-6 shadow-discord-modal animate-modal-in"
            onClick={(e) => e.stopPropagation()}
          >
            <h3 className="text-lg font-semibold text-discord-text">Remove all saved credentials?</h3>
            <p className="mt-2 text-sm text-discord-textMuted">
              All saved IdP credentials and your master password will be removed.
            </p>
            <p className="mt-1 text-sm text-discord-textMuted">
              You’ll be prompted for IdP login on each refresh until you save default credentials again. Saving them again will require a new master password.
            </p>
            <div className="mt-6 flex justify-end gap-2">
              <button
                onClick={() => setForgetAllConfirm(false)}
                className="rounded-button border border-discord-border bg-discord-darkest px-4 py-2 text-sm text-discord-textMuted hover:bg-discord-dark hover:text-discord-text transition-colors"
              >
                Cancel
              </button>
              <button
                onClick={async () => {
                  await (window.electron as { forgetAllCredentialsAndResetMasterPassword?: () => Promise<void> }).forgetAllCredentialsAndResetMasterPassword?.();
                  setForgetAllConfirm(false);
                  setMasterPasswordEnabled(false);
                  setDefaultCreds(null);
                  setDefaultUsername('');
                  setDefaultPassword('');
                  const d = await window.electron.getDefaultCredentialsDisplay();
                  setDefaultCreds(d ?? null);
                  setDefaultUsername(d?.username ?? '');
                }}
                className="rounded-button bg-discord-danger px-4 py-2 text-sm font-medium text-white hover:opacity-90 transition-opacity"
              >
                Remove all and reset
              </button>
            </div>
          </div>
        </div>
      )}

      {showCreateMasterPasswordForSave && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center modal-backdrop p-4"
          onClick={() => {
            if (createMasterPasswordSubmitting) return;
            setShowCreateMasterPasswordForSave(false);
            setCreateMasterPasswordAcknowledged(false);
            setCreateMasterPasswordHoldProgress(0);
          }}
        >
          <div
            className="w-full max-w-md rounded-card border border-discord-border bg-discord-panel shadow-discord-modal animate-modal-in overflow-hidden"
            onClick={(e) => e.stopPropagation()}
          >
            <CreateMasterPasswordModal
              acknowledged={createMasterPasswordAcknowledged}
              holdProgress={createMasterPasswordHoldProgress}
              onHoldStart={startCreateMasterPasswordHold}
              onHoldEnd={clearCreateMasterPasswordHold}
              password={createMasterPasswordValue}
              onPasswordChange={setCreateMasterPasswordValue}
              confirmPassword={createMasterPasswordConfirm}
              onConfirmPasswordChange={setCreateMasterPasswordConfirm}
              error={createMasterPasswordError}
              submitting={createMasterPasswordSubmitting}
              onSubmit={handleCreateMasterPasswordAndSave}
              submitLabel="Create & save"
              showCancel
              onCancel={() => {
                if (createMasterPasswordSubmitting) return;
                setShowCreateMasterPasswordForSave(false);
                setCreateMasterPasswordAcknowledged(false);
                setCreateMasterPasswordHoldProgress(0);
              }}
              cancelLabel="Cancel"
            />
          </div>
        </div>
      )}
    </div>
  );
}

import { useEffect, useState } from 'react';
import { HashRouter, NavLink, useLocation } from 'react-router-dom';
import { MASTER_PASSWORD_MIN_LENGTH, MASTER_PASSWORD_REQUIREMENTS, validateMasterPassword } from '../shared/masterPassword';
import Profiles from './pages/Profiles';

const IconLock = ({ className = 'w-4 h-4' }: { className?: string }) => (
  <svg className={className} fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden>
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 15v2m-6 4a2 2 0 01-2-2v-6a2 2 0 012-2h8a2 2 0 012 2v6a2 2 0 01-2 2H6zm0-8a2 2 0 01-2-2V7a2 2 0 012-2h8a2 2 0 012 2v2a2 2 0 01-2 2H6z" />
  </svg>
);
const IconLockOpen = ({ className = 'w-4 h-4' }: { className?: string }) => (
  <svg className={className} fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden>
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M8 11V7a4 4 0 118 0m-4 8v2m-6 4h12a2 2 0 002-2v-6a2 2 0 00-2-2H6a2 2 0 00-2 2v6a2 2 0 002 2z" />
  </svg>
);
const IconCheckCircle = ({ className = 'w-4 h-4' }: { className?: string }) => (
  <svg className={className} fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden>
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M9 12l2 2 4-4m6 2a9 9 0 11-18 0 9 9 0 0118 0z" />
  </svg>
);
const IconXCircle = ({ className = 'w-4 h-4' }: { className?: string }) => (
  <svg className={className} fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden>
    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M10 14l2-2m0 0l2-2m-2 2l-2-2m2 2l2 2m7-2a9 9 0 11-18 0 9 9 0 0118 0z" />
  </svg>
);
import Settings from './pages/Settings';
import TerminalScreen from './pages/TerminalScreen';
import { Tooltip } from './components/Tooltip';

function MasterPasswordGate({
  mode,
  onSuccess,
  createMasterPassword,
  unlockWithMasterPassword,
  forgetAllAndResetMasterPassword,
}: {
  mode: 'create' | 'unlock';
  onSuccess: () => void;
  createMasterPassword?: (password: string, confirm: string) => Promise<{ success: true } | { success: false; error: string }>;
  unlockWithMasterPassword?: (password: string) => Promise<{ success: true } | { success: false; error: string }>;
  forgetAllAndResetMasterPassword?: () => Promise<void>;
}) {
  const [password, setPassword] = useState('');
  const [confirmPassword, setConfirmPassword] = useState('');
  const [error, setError] = useState('');
  const [submitting, setSubmitting] = useState(false);
  const [showForgetConfirm, setShowForgetConfirm] = useState(false);
  const [forgetSubmitting, setForgetSubmitting] = useState(false);

  const handleSubmit = async () => {
    setError('');
    if (mode === 'create') {
      if (!createMasterPassword) return;
      if (password.length < 1) {
        setError('Please enter a password.');
        return;
      }
      if (password !== confirmPassword) return;
      const requirementError = validateMasterPassword(password);
      if (requirementError) {
        setError(requirementError);
        return;
      }
      setSubmitting(true);
      const result = await createMasterPassword(password, confirmPassword);
      setSubmitting(false);
      if (result.success) onSuccess();
      else setError(result.error);
    } else {
      if (!unlockWithMasterPassword) return;
      if (password.length < 1) {
        setError('Please enter your master password.');
        return;
      }
      setSubmitting(true);
      const result = await unlockWithMasterPassword(password);
      setSubmitting(false);
      if (result.success) onSuccess();
      else setError(result.error);
    }
  };

  const handleForgetAll = async () => {
    if (!forgetAllAndResetMasterPassword) return;
    setForgetSubmitting(true);
    await forgetAllAndResetMasterPassword();
    setForgetSubmitting(false);
    setShowForgetConfirm(false);
    onSuccess();
  };

  return (
    <div className="flex flex-col h-full w-full bg-discord-darkest items-center justify-center p-6">
      <div className="w-full max-w-sm rounded-card bg-discord-panel border border-discord-border p-6 shadow-discord-modal">
        <h2 className="text-lg font-semibold text-discord-text">
          {mode === 'create' ? 'Create master password' : 'Unlock saved credentials'}
        </h2>
        <p className="mt-2 text-sm text-discord-textMuted">
          {mode === 'create'
            ? 'You have saved default credentials. Set a master password to encrypt them. You will enter this password each time you open the Profile Manager.'
            : 'Enter your master password to access saved credentials.'}
        </p>
        {mode === 'create' && (
          <p className="mt-1 text-sm text-discord-textMuted">
            {MASTER_PASSWORD_REQUIREMENTS}
          </p>
        )}
        {showForgetConfirm ? (
          <div className="mt-4 space-y-3">
            <p className="text-sm text-discord-textMuted">
              This will remove all saved IdP credentials and clear your master password.
            </p>
            <p className="text-sm text-discord-textMuted">
              Until you save default credentials again, you will be prompted for your IdP username and password each time you refresh a profile. When you save default credentials again, you will set a new master password then.
            </p>
            <div className="flex gap-2">
              <button
                type="button"
                onClick={() => setShowForgetConfirm(false)}
                disabled={forgetSubmitting}
                className="flex-1 rounded-button border border-discord-border bg-discord-darkest px-4 py-2.5 text-sm font-medium text-discord-text hover:bg-discord-panel transition-colors disabled:opacity-50"
              >
                Cancel
              </button>
              <button
                type="button"
                onClick={handleForgetAll}
                disabled={forgetSubmitting}
                className="flex-1 rounded-button bg-discord-danger px-4 py-2.5 text-sm font-medium text-white hover:opacity-90 transition-colors disabled:opacity-50"
              >
                {forgetSubmitting ? 'Removing...' : 'Remove all'}
              </button>
            </div>
          </div>
        ) : (
          <>
            <div className="mt-4 space-y-3">
              <div>
                <label className="flex items-center gap-2 text-sm text-discord-textMuted">
                  {mode === 'unlock' ? <IconLockOpen className="w-4 h-4" /> : <IconLock className="w-4 h-4" />}
                  {mode === 'create' ? 'Master password' : 'Master password'}
                </label>
                <div className="relative mt-1">
                  <span className="pointer-events-none absolute left-3 top-1/2 -translate-y-1/2 text-discord-textMuted">
                    <IconLock className="w-4 h-4" />
                  </span>
                  <input
                    type="password"
                    value={password}
                    onChange={(e) => setPassword(e.target.value)}
                    onKeyDown={(e) => e.key === 'Enter' && handleSubmit()}
                    className="w-full rounded-button border border-discord-border bg-discord-darkest py-2 pl-10 pr-3 text-discord-text placeholder-discord-textMuted focus:border-discord-accent focus:outline-none"
                    placeholder={mode === 'create' ? 'Choose a password' : 'Enter password'}
                    autoFocus
                  />
                </div>
                {mode === 'create' && (
                  <p className={`mt-1.5 flex items-center gap-1.5 text-xs ${password.length >= MASTER_PASSWORD_MIN_LENGTH ? 'text-green-500' : 'text-discord-textMuted'}`}>
                    {password.length >= MASTER_PASSWORD_MIN_LENGTH ? (
                      <>
                        <IconCheckCircle className="w-3.5 h-3.5 flex-shrink-0" />
                        {MASTER_PASSWORD_MIN_LENGTH}+ characters
                      </>
                    ) : (
                      <>At least {MASTER_PASSWORD_MIN_LENGTH} characters{password.length > 0 && ` (${password.length}/${MASTER_PASSWORD_MIN_LENGTH})`}</>
                    )}
                  </p>
                )}
              </div>
              {mode === 'create' && (
                <div>
                  <label className="flex items-center gap-2 text-sm text-discord-textMuted">
                    <IconLock className="w-4 h-4" />
                    Confirm master password
                  </label>
                  <input
                    type="password"
                    value={confirmPassword}
                    onChange={(e) => setConfirmPassword(e.target.value)}
                    onKeyDown={(e) => e.key === 'Enter' && handleSubmit()}
                    className="mt-1 w-full rounded-button border border-discord-border bg-discord-darkest px-3 py-2 text-discord-text placeholder-discord-textMuted focus:border-discord-accent focus:outline-none"
                    placeholder="Confirm password"
                  />
                  {confirmPassword.length > 0 && (
                    <p className={`mt-1.5 flex items-center gap-1.5 text-xs ${password === confirmPassword ? 'text-green-500' : 'text-discord-danger'}`}>
                      {password === confirmPassword ? (
                        <>
                          <IconCheckCircle className="w-3.5 h-3.5 flex-shrink-0" />
                          Passwords match
                        </>
                      ) : (
                        <>
                          <IconXCircle className="w-3.5 h-3.5 flex-shrink-0" />
                          Passwords don&apos;t match
                        </>
                      )}
                    </p>
                  )}
                </div>
              )}
            </div>
            {error && <p className="mt-3 text-sm text-discord-danger">{error}</p>}
            <div className="mt-6 space-y-2">
              <button
                type="button"
                onClick={handleSubmit}
                disabled={submitting}
                className="w-full rounded-button bg-discord-accent px-4 py-2.5 text-sm font-medium text-white hover:bg-discord-accentHover disabled:opacity-50 transition-colors"
              >
                {submitting ? 'Please wait...' : mode === 'create' ? 'Create master password' : 'Unlock'}
              </button>
              {mode === 'unlock' && forgetAllAndResetMasterPassword && (
                <button
                  type="button"
                  onClick={() => setShowForgetConfirm(true)}
                  className="w-full rounded-button border border-discord-border bg-transparent px-4 py-2 text-sm text-discord-textMuted hover:text-discord-text hover:bg-discord-darkest/50 transition-colors"
                >
                  Forgot master password?
                </button>
              )}
            </div>
          </>
        )}
      </div>
    </div>
  );
}

type UpdateStatus =
  | { type: 'available'; version: string }
  | { type: 'downloading'; percent: number }
  | { type: 'downloaded'; version: string }
  | { type: 'error'; message: string }
  | { type: 'no-update' };

const GITHUB_REPO_URL = 'https://github.com/MichaelGermino/AWSProfileManager';

/** Renders all main screens so they stay mounted; only the active route is visible. Terminal state persists when navigating away and back. */
function PersistentMainContent() {
  const location = useLocation();
  const path = location.pathname || '/';

  return (
    <main className="flex-1 flex flex-col min-w-0 min-h-0 overflow-hidden">
      <div
        className={path === '/' ? 'flex-1 overflow-auto p-8' : 'hidden'}
        aria-hidden={path !== '/'}
      >
        <Profiles />
      </div>
      <div
        className={path === '/settings' ? 'flex-1 overflow-auto p-8' : 'hidden'}
        aria-hidden={path !== '/settings'}
      >
        <Settings />
      </div>
      <div
        className={path === '/terminal' ? 'flex-1 flex flex-col min-h-0 overflow-hidden' : 'hidden'}
        aria-hidden={path !== '/terminal'}
      >
        <TerminalScreen isVisible={path === '/terminal'} />
      </div>
    </main>
  );
}

type MasterPasswordState = 'loading' | 'needsCreate' | 'needsUnlock' | 'unlocked';

function App() {
  const [updateStatus, setUpdateStatus] = useState<UpdateStatus | null>(null);
  const [appIconDataUrl, setAppIconDataUrl] = useState<string | null>(null);
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
  const [masterPasswordState, setMasterPasswordState] = useState<MasterPasswordState>('loading');
  const platform = window.electron?.platform ?? '';

  useEffect(() => {
    const check = async () => {
      const status = await (window.electron as { getMasterPasswordStatus?: () => Promise<{ needsUnlock?: true; needsCreateMasterPassword?: true; unlocked?: true }> }).getMasterPasswordStatus?.();
      if (!status) {
        setMasterPasswordState('unlocked');
        return;
      }
      if (status.needsCreateMasterPassword) setMasterPasswordState('needsCreate');
      else if (status.needsUnlock) setMasterPasswordState('needsUnlock');
      else setMasterPasswordState('unlocked');
    };
    check();
  }, []);

  useEffect(() => {
    window.electron?.getSidebarCollapsed?.()?.then((collapsed: boolean) => setSidebarCollapsed(collapsed));
  }, []);

  useEffect(() => {
    const remove = (window.electron as { onMasterPasswordReset?: (cb: () => void) => () => void }).onMasterPasswordReset?.(() => setMasterPasswordState('unlocked'));
    return () => remove?.();
  }, []);

  const setSidebarCollapsedAndPersist = (collapsed: boolean) => {
    setSidebarCollapsed(collapsed);
    window.electron?.setSidebarCollapsed?.(collapsed);
  };

  useEffect(() => {
    window.electron.onUpdateStatus((status) => setUpdateStatus(status as UpdateStatus));
  }, []);

  useEffect(() => {
    window.electron?.getAppIconDataUrl?.()?.then((url: string | null) => url && setAppIconDataUrl(url));
  }, []);

  const handleInstallUpdate = () => {
    window.electron.installUpdateAndRestart();
  };

  const handleRetryUpdate = () => {
    (window.electron as { checkForUpdates?: () => Promise<unknown> }).checkForUpdates?.();
  };

  const electronWithMaster = window.electron as {
    getMasterPasswordStatus?: () => Promise<{ needsUnlock?: true; needsCreateMasterPassword?: true; unlocked?: true }>;
    createMasterPassword?: (password: string, confirm: string) => Promise<{ success: true } | { success: false; error: string }>;
    unlockWithMasterPassword?: (password: string) => Promise<{ success: true } | { success: false; error: string }>;
  };

  if (masterPasswordState === 'loading') {
    return (
      <div className="flex flex-col h-full w-full bg-discord-darkest items-center justify-center">
        <p className="text-discord-textMuted">Loading...</p>
      </div>
    );
  }

  if (masterPasswordState === 'needsCreate' || masterPasswordState === 'needsUnlock') {
    return (
      <MasterPasswordGate
        mode={masterPasswordState === 'needsCreate' ? 'create' : 'unlock'}
        onSuccess={() => setMasterPasswordState('unlocked')}
        createMasterPassword={electronWithMaster.createMasterPassword}
        unlockWithMasterPassword={electronWithMaster.unlockWithMasterPassword}
        forgetAllAndResetMasterPassword={
          masterPasswordState === 'needsUnlock'
            ? (window.electron as { forgetAllCredentialsAndResetMasterPassword?: () => Promise<void> }).forgetAllCredentialsAndResetMasterPassword
            : undefined
        }
      />
    );
  }

  return (
    <HashRouter>
      <div className="flex flex-col h-full w-full bg-discord-darkest">
        {platform === 'win32' && (
          <div
            className="flex items-center h-10 flex-shrink-0 bg-discord-sidebar border-b border-discord-border"
            style={{ WebkitAppRegion: 'drag' } as React.CSSProperties}
          >
            <div className="flex items-center gap-2 pl-3 min-w-0 flex-1">
              {appIconDataUrl ? (
                <img src={appIconDataUrl} alt="" className="h-5 w-5 flex-shrink-0 object-contain" aria-hidden />
              ) : (
                <svg className="h-5 w-5 flex-shrink-0 text-discord-textMuted" fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden>
                  <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M4 6h16M4 12h16M4 18h16" />
                </svg>
              )}
              <span className="text-sm font-semibold text-discord-text truncate">AWS Profile Manager</span>
            </div>
            <div className="flex items-center h-full" style={{ WebkitAppRegion: 'no-drag' } as React.CSSProperties}>
              <Tooltip label="Open GitHub repository">
                <a
                  href={GITHUB_REPO_URL}
                  target="_blank"
                  rel="noopener noreferrer"
                  onClick={(e) => {
                    e.preventDefault();
                    window.electron.openExternal?.(GITHUB_REPO_URL);
                  }}
                  className="p-2 text-discord-textMuted hover:text-discord-text hover:bg-discord-darkest/60 transition-colors inline-flex rounded-none"
                  aria-label="Open GitHub repository"
                >
                  <svg className="h-5 w-5" fill="currentColor" viewBox="0 0 24 24" aria-hidden>
                    <path fillRule="evenodd" d="M12 2C6.477 2 2 6.484 2 12.017c0 4.425 2.865 8.18 6.839 9.504.5.092.682-.217.682-.483 0-.237-.008-.868-.013-1.703-2.782.605-3.369-1.343-3.369-1.343-.454-1.158-1.11-1.466-1.11-1.466-.908-.62.069-.608.069-.608 1.003.07 1.531 1.032 1.531 1.032.892 1.53 2.341 1.088 2.91.832.092-.647.35-1.088.636-1.338-2.22-.253-4.555-1.113-4.555-4.951 0-1.093.39-1.988 1.029-2.688-.103-.253-.446-1.272.098-2.65 0 0 .84-.27 2.75 1.026A9.564 9.564 0 0112 6.844c.85.004 1.705.115 2.504.337 1.909-1.296 2.747-1.027 2.747-1.027.546 1.379.202 2.398.1 2.651.64.7 1.028 1.595 1.028 2.688 0 3.848-2.339 4.695-4.566 4.943.359.309.678.92.678 1.855 0 1.338-.012 2.419-.012 2.747 0 .268.18.58.688.482A10.019 10.019 0 0022 12.017C22 6.484 17.522 2 12 2z" clipRule="evenodd" />
                  </svg>
                </a>
              </Tooltip>
              {updateStatus?.type === 'downloaded' && (
                <Tooltip label="Update ready!">
                  <button
                    type="button"
                    onClick={handleInstallUpdate}
                    className="rounded-md p-1.5 text-discord-success hover:bg-discord-darkest/60 hover:text-discord-success transition-colors inline-flex"
                    aria-label="Update ready!"
                  >
                    <svg className="h-5 w-5" fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden>
                      <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M4 16v1a3 3 0 003 3h10a3 3 0 003-3v-1m-4-4l-4 4m0 0l-4-4m4 4V4" />
                    </svg>
                  </button>
                </Tooltip>
              )}
              {updateStatus?.type === 'error' && (
                <Tooltip label={updateStatus.message} placement="below" wrap>
                  <button
                    type="button"
                    onClick={handleRetryUpdate}
                    className="rounded-md p-1.5 text-discord-danger hover:bg-discord-darkest/60 hover:text-discord-danger transition-colors inline-flex"
                    aria-label="Update download failed; click to retry"
                  >
                    <svg className="h-5 w-5" fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden>
                      <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 9v2m0 4h.01m-6.938 4h13.856c1.54 0 2.502-1.667 1.732-3L13.732 4c-.77-1.333-2.694-1.333-3.464 0L3.34 16c-.77 1.333.192 3 1.732 3z" />
                    </svg>
                  </button>
                </Tooltip>
              )}
              <span className="w-px h-4 bg-discord-border mx-1" aria-hidden />
              <Tooltip label="Minimize">
              <button
                type="button"
                onClick={() => window.electron.windowMinimize?.()}
                className="p-2 text-discord-textMuted hover:text-discord-text hover:bg-discord-darkest/60 transition-colors rounded-none"
                aria-label="Minimize"
              >
                <svg className="h-4 w-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                  <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M20 12H4" />
                </svg>
              </button>
              </Tooltip>
              <Tooltip label="Maximize">
              <button
                type="button"
                onClick={() => window.electron.windowMaximize?.()}
                className="p-2 text-discord-textMuted hover:text-discord-text hover:bg-discord-darkest/60 transition-colors rounded-none"
                aria-label="Maximize"
              >
                <svg className="h-4 w-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                  <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M4 6h16v12H4V6z" />
                </svg>
              </button>
              </Tooltip>
              <Tooltip label="Close">
              <button
                type="button"
                onClick={() => window.electron.windowClose?.()}
                className="p-2 text-discord-textMuted hover:text-white hover:bg-discord-danger transition-colors rounded-none"
                aria-label="Close"
              >
                <svg className="h-4 w-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                  <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M6 18L18 6M6 6l12 12" />
                </svg>
              </button>
              </Tooltip>
            </div>
          </div>
        )}
        <div className="flex flex-1 min-h-0">
        <aside
          className={`flex-shrink-0 bg-discord-sidebar flex flex-col border-r border-discord-border transition-[width] duration-200 ease-out ${sidebarCollapsed ? 'w-[72px]' : 'w-72'}`}
        >
          {sidebarCollapsed ? (
            <>
              <div className="flex flex-col items-center pt-2 pb-1">
                <Tooltip label="Expand sidebar" placement="right">
                  <button
                    type="button"
                    onClick={() => setSidebarCollapsedAndPersist(false)}
                    className="flex items-center justify-center w-12 h-12 rounded-2xl flex-shrink-0 text-discord-textMuted hover:bg-discord-panel hover:text-discord-text transition-colors"
                    aria-label="Expand sidebar"
                  >
                    <svg className="h-6 w-6" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2} strokeLinecap="round" strokeLinejoin="round">
                      <rect x={2} y={2} width={6} height={20} rx={1} />
                      <rect x={10} y={2} width={12} height={20} rx={1} />
                    </svg>
                  </button>
                </Tooltip>
              </div>
              <div className="mx-2 my-1 h-px bg-discord-border" role="separator" />
              <nav className="flex-1 flex flex-col items-center py-2 gap-1 min-h-0">
                <Tooltip label="Profiles" placement="right">
                  <NavLink
                    to="/"
                    className={({ isActive }) =>
                      `flex items-center justify-center w-12 h-12 rounded-2xl flex-shrink-0 transition-all duration-200 ${
                        isActive
                          ? 'bg-discord-accent text-white shadow-discord-accent'
                          : 'text-discord-textMuted hover:bg-discord-panel hover:text-discord-textMutedHover'
                      }`
                    }
                  >
                    <svg className="h-6 w-6" fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden>
                      <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M17 20h5v-2a3 3 0 00-5.356-1.857M17 20H7m10 0v-2c0-.656-.126-1.283-.356-1.857M7 20H2v-2a3 3 0 015.356-1.857M7 20v-2c0-.656.126-1.283.356-1.857m0 0a5.002 5.002 0 019.288 0M15 7a3 3 0 11-6 0 3 3 0 016 0zm6 3a2 2 0 11-4 0 2 2 0 014 0zM7 10a2 2 0 11-4 0 2 2 0 014 0z" />
                    </svg>
                  </NavLink>
                </Tooltip>
                <Tooltip label="Terminal" placement="right">
                  <NavLink
                    to="/terminal"
                    className={({ isActive }) =>
                      `flex items-center justify-center w-12 h-12 rounded-2xl flex-shrink-0 transition-all duration-200 ${
                        isActive
                          ? 'bg-discord-accent text-white shadow-discord-accent'
                          : 'text-discord-textMuted hover:bg-discord-panel hover:text-discord-textMutedHover'
                      }`
                    }
                  >
                    <svg className="h-6 w-6" fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden>
                      <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M8 9l3 3-3 3m5 0h3M5 20h14a2 2 0 002-2V6a2 2 0 00-2-2H5a2 2 0 00-2 2v12a2 2 0 002 2z" />
                    </svg>
                  </NavLink>
                </Tooltip>
                <Tooltip label="Settings" placement="right">
                  <NavLink
                    to="/settings"
                    className={({ isActive }) =>
                      `flex items-center justify-center w-12 h-12 rounded-2xl flex-shrink-0 transition-all duration-200 ${
                        isActive
                          ? 'bg-discord-accent text-white shadow-discord-accent'
                          : 'text-discord-textMuted hover:bg-discord-panel hover:text-discord-textMutedHover'
                      }`
                    }
                  >
                    <svg className="h-6 w-6" fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden>
                      <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z" />
                      <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
                    </svg>
                  </NavLink>
                </Tooltip>
              </nav>
            </>
          ) : (
            <>
              <div className="px-3 py-3 flex items-center border-b border-discord-border">
                <Tooltip label="Collapse sidebar" placement="right">
                  <button
                    type="button"
                    onClick={() => setSidebarCollapsedAndPersist(true)}
                    className="flex-shrink-0 p-1.5 rounded-lg text-discord-textMuted hover:bg-discord-panel hover:text-discord-text transition-colors"
                    aria-label="Collapse sidebar"
                  >
                    <svg className="h-5 w-5" fill="none" stroke="currentColor" viewBox="0 0 24 24" strokeWidth={2} strokeLinecap="round" strokeLinejoin="round">
                      <rect x={2} y={2} width={6} height={20} rx={1} />
                      <rect x={10} y={2} width={12} height={20} rx={1} />
                    </svg>
                  </button>
                </Tooltip>
              </div>
              <nav className="flex-1 px-3 py-3 space-y-1">
                <NavLink
                  to="/"
                  className={({ isActive }) =>
                    `flex items-center gap-3 rounded-lg px-3 py-2.5 text-sm font-medium transition-all duration-200 relative ${
                      isActive
                        ? 'bg-discord-accent text-white shadow-discord-accent'
                        : 'text-discord-textMuted hover:bg-discord-panel hover:text-discord-textMutedHover'
                    } ${isActive ? "before:content-[''] before:absolute before:left-0 before:top-1/2 before:-translate-y-1/2 before:w-0.5 before:h-6 before:rounded-r before:bg-white before:opacity-90" : ''}`
                  }
                >
                  <svg className="h-5 w-5 flex-shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden>
                    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M17 20h5v-2a3 3 0 00-5.356-1.857M17 20H7m10 0v-2c0-.656-.126-1.283-.356-1.857M7 20H2v-2a3 3 0 015.356-1.857M7 20v-2c0-.656.126-1.283.356-1.857m0 0a5.002 5.002 0 019.288 0M15 7a3 3 0 11-6 0 3 3 0 016 0zm6 3a2 2 0 11-4 0 2 2 0 014 0zM7 10a2 2 0 11-4 0 2 2 0 014 0z" />
                  </svg>
                  Profiles
                </NavLink>
                <NavLink
                  to="/terminal"
                  className={({ isActive }) =>
                    `flex items-center gap-3 rounded-lg px-3 py-2.5 text-sm font-medium transition-all duration-200 relative ${
                      isActive
                        ? 'bg-discord-accent text-white shadow-discord-accent'
                        : 'text-discord-textMuted hover:bg-discord-panel hover:text-discord-textMutedHover'
                    } ${isActive ? "before:content-[''] before:absolute before:left-0 before:top-1/2 before:-translate-y-1/2 before:w-0.5 before:h-6 before:rounded-r before:bg-white before:opacity-90" : ''}`
                  }
                >
                  <svg className="h-5 w-5 flex-shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden>
                    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M8 9l3 3-3 3m5 0h3M5 20h14a2 2 0 002-2V6a2 2 0 00-2-2H5a2 2 0 00-2 2v12a2 2 0 002 2z" />
                  </svg>
                  Terminal
                </NavLink>
                <NavLink
                  to="/settings"
                  className={({ isActive }) =>
                    `flex items-center gap-3 rounded-lg px-3 py-2.5 text-sm font-medium transition-all duration-200 relative ${
                      isActive
                        ? 'bg-discord-accent text-white shadow-discord-accent'
                        : 'text-discord-textMuted hover:bg-discord-panel hover:text-discord-textMutedHover'
                    } ${isActive ? "before:content-[''] before:absolute before:left-0 before:top-1/2 before:-translate-y-1/2 before:w-0.5 before:h-6 before:rounded-r before:bg-white before:opacity-90" : ''}`
                  }
                >
                  <svg className="h-5 w-5 flex-shrink-0" fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden>
                    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M10.325 4.317c.426-1.756 2.924-1.756 3.35 0a1.724 1.724 0 002.573 1.066c1.543-.94 3.31.826 2.37 2.37a1.724 1.724 0 001.065 2.572c1.756.426 1.756 2.924 0 3.35a1.724 1.724 0 00-1.066 2.573c.94 1.543-.826 3.31-2.37 2.37a1.724 1.724 0 00-2.572 1.065c-.426 1.756-2.924 1.756-3.35 0a1.724 1.724 0 00-2.573-1.066c-1.543.94-3.31-.826-2.37-2.37a1.724 1.724 0 00-1.065-2.572c-1.756-.426-1.756-2.924 0-3.35a1.724 1.724 0 001.066-2.573c-.94-1.543.826-3.31 2.37-2.37.996.608 2.296.07 2.572-1.065z" />
                    <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M15 12a3 3 0 11-6 0 3 3 0 016 0z" />
                  </svg>
                  Settings
                </NavLink>
              </nav>
            </>
          )}
        </aside>
        <div className="flex-1 flex flex-col min-w-0">
          <PersistentMainContent />
        </div>
        </div>
      </div>
    </HashRouter>
  );
}

export default App;

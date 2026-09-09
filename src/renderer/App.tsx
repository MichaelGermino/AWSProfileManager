import { lazy, Suspense, useEffect, useRef, useState } from 'react';
import { HashRouter, NavLink, useLocation } from 'react-router';
import { validateMasterPassword } from '../shared/masterPassword';
import { CreateMasterPasswordModal } from './components/CreateMasterPasswordModal';
import Profiles from './pages/Profiles';
import { SetupWizard, type WizardMode } from './wizard/SetupWizard';

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
import Settings from './pages/Settings';
// Lazy so the xterm + markdown chunks stay out of the initial load graph; see PersistentMainContent.
// Shared with the idle prefetch below so both resolve the same module — the second call is a cache
// hit, which is what makes the first visit to Terminal instant.
const importTerminalScreen = () => import('./pages/TerminalScreen');
const TerminalScreen = lazy(importTerminalScreen);
// Lazy: pulls in react-markdown/remark-gfm, and this shows at most once per version.
const ChangelogModal = lazy(() => import('./components/ChangelogModal'));
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
  const [createAcknowledged, setCreateAcknowledged] = useState(false);
  const [holdProgress, setHoldProgress] = useState(0);
  const holdRafRef = useRef<number | null>(null);
  const holdStartRef = useRef<number>(0);

  const clearHold = () => {
    if (holdRafRef.current != null) {
      cancelAnimationFrame(holdRafRef.current);
      holdRafRef.current = null;
    }
    setHoldProgress(0);
  };

  const startHold = () => {
    if (holdRafRef.current != null) return;
    holdStartRef.current = Date.now();
    const tick = () => {
      const elapsed = Date.now() - holdStartRef.current;
      const pct = Math.min(100, (elapsed / 1000) * 100);
      setHoldProgress(pct);
      if (pct >= 100) {
        if (holdRafRef.current != null) cancelAnimationFrame(holdRafRef.current);
        holdRafRef.current = null;
        setHoldProgress(0);
        setCreateAcknowledged(true);
        return;
      }
      holdRafRef.current = requestAnimationFrame(tick);
    };
    holdRafRef.current = requestAnimationFrame(tick);
  };

  useEffect(() => () => { if (holdRafRef.current != null) cancelAnimationFrame(holdRafRef.current); }, []);

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
      {mode === 'create' && !showForgetConfirm ? (
        <div className="fixed inset-0 z-50 flex items-center justify-center bg-discord-darkest p-4">
          <div
            className="w-full max-w-md rounded-card border border-discord-border bg-discord-panel shadow-discord-modal animate-modal-in overflow-hidden"
            onClick={(e) => e.stopPropagation()}
          >
            <CreateMasterPasswordModal
              acknowledged={createAcknowledged}
              holdProgress={holdProgress}
              onHoldStart={startHold}
              onHoldEnd={clearHold}
              password={password}
              onPasswordChange={setPassword}
              confirmPassword={confirmPassword}
              onConfirmPasswordChange={setConfirmPassword}
              error={error}
              submitting={submitting}
              onSubmit={handleSubmit}
              submitLabel="Create master password"
            />
          </div>
        </div>
      ) : (
      <div className="w-full max-w-sm rounded-card bg-discord-panel border border-discord-border p-6 shadow-discord-modal">
        <h2 className="text-lg font-semibold text-discord-text">
          {showForgetConfirm ? 'Remove all credentials?' : 'Unlock saved credentials'}
        </h2>
        {!showForgetConfirm && (
          <p className="mt-2 text-sm text-discord-textMuted">
            Enter your master password to access saved credentials.
          </p>
        )}
        {showForgetConfirm ? (
          <div className="mt-4 space-y-3">
            <p className="text-sm text-discord-textMuted">
              All saved IdP credentials and your master password will be removed.
            </p>
            <p className="text-sm text-discord-textMuted">
              You’ll be prompted for IdP login on each refresh until you save default credentials again. Saving them again will require a new master password.
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
                  <IconLockOpen className="w-4 h-4" />
                  Master password
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
                    placeholder="Enter password"
                    autoFocus
                  />
                </div>
              </div>
            </div>
            {error && <p className="mt-3 text-sm text-discord-danger">{error}</p>}
            <div className="mt-6 space-y-2">
              <button
                type="button"
                onClick={handleSubmit}
                disabled={submitting}
                className="w-full rounded-button bg-discord-accent px-4 py-2.5 text-sm font-medium text-white hover:bg-discord-accentHover disabled:opacity-50 transition-colors"
              >
                {submitting ? 'Please wait...' : 'Unlock'}
              </button>
              {forgetAllAndResetMasterPassword && (
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
      )}
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

/**
 * Stand-in shown while the terminal chunk resolves.
 *
 * Mirrors TerminalScreen's own frame — same height calc, same top-bar strip — so the real screen
 * swaps in without the layout jumping. With the idle prefetch in place this is usually skipped
 * entirely; it exists for a cold first visit that beats the prefetch.
 */
function TerminalSkeleton() {
  return (
    <div className="flex flex-col bg-discord-darkest h-[calc(100vh-6rem)] max-h-[calc(100vh-6rem)] overflow-hidden">
      <div className="flex items-center gap-3 border-b border-discord-border bg-discord-panel px-3 py-2">
        <div className="h-4 w-28 animate-pulse rounded bg-discord-borderLight/60" />
        <div className="h-7 w-44 animate-pulse rounded-button bg-discord-borderLight/40" />
        <div className="ml-auto h-7 w-24 animate-pulse rounded-button bg-discord-borderLight/40" />
      </div>
      <div className="min-h-0 flex-1 p-2">
        <div className="h-full rounded-panel bg-discord-content p-4">
          <div className="flex items-center gap-2 font-mono text-sm text-discord-textMuted">
            <span className="inline-block h-3.5 w-3.5 animate-spin rounded-full border-2 border-discord-borderLight border-t-discord-accent" />
            Starting terminal…
          </div>
          <div className="mt-4 space-y-2.5">
            {['w-2/5', 'w-3/5', 'w-1/3'].map((w) => (
              <div key={w} className={`h-3 ${w} animate-pulse rounded bg-discord-borderLight/25`} />
            ))}
          </div>
        </div>
      </div>
    </div>
  );
}

/** Renders all main screens so they stay mounted; only the active route is visible. Terminal state persists when navigating away and back. */
function PersistentMainContent() {
  const location = useLocation();
  const path = location.pathname || '/';

  /**
   * The terminal mounts once and stays mounted, so its PTY and scrollback survive navigating away
   * and back.
   *
   * It mounts on whichever comes first: visiting the screen, or the idle warm-up below. What must
   * NOT happen is mounting during startup — the xterm (~330 kB) and markdown (~165 kB) chunks plus
   * the PowerShell spawn measured dom-ready at 4367ms when they landed before first paint (see
   * startup-log.txt). Idle keeps that win while still having the screen ready before it is asked for.
   */
  const [terminalMounted, setTerminalMounted] = useState(false);
  const onTerminal = path === '/terminal';
  useEffect(() => {
    if (onTerminal) setTerminalMounted(true);
  }, [onTerminal]);

  /**
   * Once the app goes idle, load the terminal chunk and then mount it hidden, so the first visit
   * to Terminal has nothing left to do and simply appears.
   *
   * Idle rather than eager is the whole trick: mounting at startup is what previously pushed
   * dom-ready to 4367ms, because xterm, markdown and the PowerShell spawn all landed before first
   * paint. Waiting for idle keeps startup identical and moves the cost to a moment when nothing
   * is competing for the main thread.
   *
   * Mounting hidden is safe: EmbeddedTerminal skips the initial fit when its container has no
   * layout and lets the isVisible effect do the first real fit. That path already ran every time
   * you navigated away and back — this just makes it the first case too.
   *
   * The chunk is awaited before mounting so Suspense never has to fall back, even invisibly.
   */
  useEffect(() => {
    if (terminalMounted) return;
    let cancelled = false;
    const warm = () => {
      void importTerminalScreen().then(() => {
        if (!cancelled) setTerminalMounted(true);
      });
    };
    const idle = window.requestIdleCallback;
    if (typeof idle === 'function') {
      const handle = idle(warm, { timeout: 3000 });
      return () => {
        cancelled = true;
        window.cancelIdleCallback?.(handle);
      };
    }
    const timer = window.setTimeout(warm, 1500);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [terminalMounted]);

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
        <Settings isVisible={path === '/settings'} />
      </div>
      <div
        className={onTerminal ? 'flex-1 flex flex-col min-h-0 overflow-hidden' : 'hidden'}
        aria-hidden={!onTerminal}
      >
        {terminalMounted && (
          <Suspense fallback={<TerminalSkeleton />}>
            <TerminalScreen isVisible={onTerminal} />
          </Suspense>
        )}
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
  const [changelog, setChangelog] = useState<{ version: string; notes: string; url: string } | null>(null);
  /** null = closed. Opened automatically on first run, or on demand from the Profiles page. */
  const [wizard, setWizard] = useState<WizardMode | null>(null);
  const [showPauseMessageAfterUnlock, setShowPauseMessageAfterUnlock] = useState(false);
  const [pauseMessageFading, setPauseMessageFading] = useState(false);
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

  const pauseMessageRemoveRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(() => {
    if (!showPauseMessageAfterUnlock) return;
    setPauseMessageFading(false);
    const timer = setTimeout(() => {
      setPauseMessageFading(true);
      pauseMessageRemoveRef.current = setTimeout(() => setShowPauseMessageAfterUnlock(false), 300);
    }, 5000);
    return () => {
      clearTimeout(timer);
      if (pauseMessageRemoveRef.current) {
        clearTimeout(pauseMessageRemoveRef.current);
        pauseMessageRemoveRef.current = null;
      }
    };
  }, [showPauseMessageAfterUnlock]);

  const handleInstallUpdate = () => {
    window.electron.installUpdateAndRestart();
  };

  const handleRetryUpdate = () => {
    (window.electron as { checkForUpdates?: () => Promise<unknown> }).checkForUpdates?.();
  };

  /**
   * Show the wizard on first run only: no profiles AND setup never completed. Requiring both means
   * an existing user upgrading into this version is never interrupted, even though their
   * settings.json predates the flag. Runs after the master-password gate resolves so the two
   * full-screen surfaces never fight.
   */
  useEffect(() => {
    if (masterPasswordState !== 'unlocked') return;
    let cancelled = false;
    void (async () => {
      const [settings, profiles] = await Promise.all([
        window.electron.getSettings(),
        window.electron.getProfiles(),
      ]);
      if (cancelled) return;
      if (!settings?.setupCompleted && (profiles?.length ?? 0) === 0) setWizard('firstRun');
    })();
    return () => {
      cancelled = true;
    };
  }, [masterPasswordState]);

  /**
   * Release notes for this version, shown once after an update.
   *
   * Gated on 'unlocked' so it can never appear over the master-password screen, and so the GitHub
   * call does not compete with startup. Main decides whether anything is due — no notes, no
   * release, or already seen all resolve to null — so there is nothing to decide here.
   */
  useEffect(() => {
    if (masterPasswordState !== 'unlocked') return;
    let cancelled = false;
    void window.electron?.getPendingChangelog?.().then((pending) => {
      if (!cancelled && pending) setChangelog(pending);
    });
    return () => {
      cancelled = true;
    };
  }, [masterPasswordState]);

  const dismissChangelog = () => {
    if (changelog) void window.electron?.markChangelogSeen?.(changelog.version);
    setChangelog(null);
  };

  // The Profiles page asks for the import wizard through a window event, so App owns the one
  // instance and the overlay always covers the whole window.
  useEffect(() => {
    const open = () => setWizard('import');
    window.addEventListener('wizard:openImport', open);
    return () => window.removeEventListener('wizard:openImport', open);
  }, []);

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


  const handleUnlockSuccess = () => {
    setMasterPasswordState('unlocked');
    const electron = window.electron as {
      getRefreshPaused?: () => Promise<{ paused: boolean; pausedDueToFailures: boolean }>;
      refreshAutoRefreshProfiles?: () => Promise<void>;
    };
    void (async () => {
      try {
        const state = await electron.getRefreshPaused?.();
        if (state?.paused) {
          setShowPauseMessageAfterUnlock(true);
        } else {
          electron.refreshAutoRefreshProfiles?.();
        }
      } catch {
        // ignore
      }
    })();
  };

  if (masterPasswordState === 'needsCreate' || masterPasswordState === 'needsUnlock') {
    return (
      <MasterPasswordGate
        mode={masterPasswordState === 'needsCreate' ? 'create' : 'unlock'}
        onSuccess={handleUnlockSuccess}
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
      {wizard && (
        <SetupWizard
          mode={wizard}
          onClose={async (createdAny) => {
            setWizard(null);
            const settings = await window.electron.getSettings();
            if (!settings?.setupCompleted) {
              await window.electron.saveSettings({ ...settings, setupCompleted: true });
            }
            // Creating profiles changes the list the Profiles page already rendered.
            if (createdAny) window.dispatchEvent(new Event('profiles:changed'));
          }}
        />
      )}
      <div className="flex flex-col h-full w-full bg-discord-darkest">
        {showPauseMessageAfterUnlock && (
          <div
            className={`fixed bottom-4 left-1/2 -translate-x-1/2 z-50 flex items-center gap-3 rounded-button border border-discord-border bg-discord-panel px-4 py-3 shadow-discord-modal text-sm text-discord-text transition-opacity duration-300 ${pauseMessageFading ? 'opacity-0' : 'opacity-100'}`}
          >
            <span>
              Pause auto refresh is enabled. Profiles were not refreshed. Resume in Settings.
            </span>
            <button
              type="button"
              onClick={() => setShowPauseMessageAfterUnlock(false)}
              className="flex-shrink-0 p-1 rounded hover:bg-discord-darkest text-discord-textMuted hover:text-discord-text transition-colors"
              aria-label="Dismiss"
            >
              <svg className="w-4 h-4" fill="none" stroke="currentColor" viewBox="0 0 24 24">
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M6 18L18 6M6 6l12 12" />
              </svg>
            </button>
          </div>
        )}
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
      {changelog && (
        <Suspense fallback={null}>
          <ChangelogModal
            version={changelog.version}
            notes={changelog.notes}
            url={changelog.url}
            onClose={dismissChangelog}
          />
        </Suspense>
      )}
    </HashRouter>
  );
}

export default App;

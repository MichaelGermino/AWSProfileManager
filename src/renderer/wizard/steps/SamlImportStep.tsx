import { useEffect, useState } from 'react';
import type { AwsRole } from '../../../shared/types';
import { AccountPicker } from '../AccountPicker';
import { fromSamlRoles, type ImportableAccount } from '../importModel';
import { StepError, StepFooter, StepHeader, type StepProps } from '../SetupWizard';
import { persistImport } from '../persistImport';

/** Sign-in posts through the IdP redirect chain and then asks AWS for account names, so it is
 *  genuinely slow — several seconds is normal. Say so rather than looking frozen. */
function Spinner({ label }: { label: string }) {
  return (
    <div className="mt-4 flex items-center gap-3 text-sm text-discord-textMuted" role="status" aria-live="polite">
      <svg className="h-4 w-4 animate-spin text-discord-accent" viewBox="0 0 24 24" fill="none" aria-hidden>
        <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
        <path className="opacity-90" fill="currentColor" d="M4 12a8 8 0 018-8v4a4 4 0 00-4 4H4z" />
      </svg>
      {label}
    </div>
  );
}

/**
 * Bulk import of SAML roles — the counterpart to the Identity Center importer.
 *
 * fetchRoles only works when credentials are stored; otherwise it returns credentialsRequired, so
 * this falls back to fetchRolesWithCredentials with a one-off prompt. That keeps SAML import usable
 * for someone who skipped the master password and credentials steps.
 */
export function SamlImportStep({ next, skip, back, addCreated }: StepProps) {
  const [idpUrl, setIdpUrl] = useState('');
  const [accounts, setAccounts] = useState<ImportableAccount[] | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [needCreds, setNeedCreds] = useState(false);
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [displayNames, setDisplayNames] = useState<Record<string, string>>({});
  /** Default on: having just typed them, the user almost always wants them kept. */
  const [remember, setRemember] = useState(true);
  const [canRemember, setCanRemember] = useState(false);
  const [savedNotice, setSavedNotice] = useState(false);
  const [phase, setPhase] = useState<string | null>(null);

  useEffect(() => {
    window.electron.getSettings().then((s) => {
      setIdpUrl(s?.defaultIdpEntryUrl ?? '');
      setDisplayNames(s?.accountDisplayNames ?? {});
    });
    // Saving credentials encrypts them with the master password, so it is only offered when one exists.
    window.electron.getMasterPasswordEnabled().then(setCanRemember);
    // Show the sign-in fields immediately when nothing is stored, instead of making the user click
    // "Load roles" only to be told credentials are required.
    window.electron.getDefaultCredentialsDisplay().then((d) => {
      if (!d?.hasPassword) {
        setNeedCreds(true);
        setUsername(d?.locked ? '' : (d?.username ?? ''));
      }
    });
  }, []);

  const receive = (roles: AwsRole[]) => {
    const mapped = fromSamlRoles(roles, displayNames);
    setAccounts(mapped);
    setNeedCreds(false);
    if (mapped.length === 0) setError('No roles were returned for this IdP URL.');
  };

  const load = async () => {
    if (!idpUrl.trim()) {
      setError('Enter your IdP entry URL first.');
      return;
    }
    setBusy(true);
    setError(null);
    setPhase('Signing in to your identity provider…');
    try {
      const result = (await window.electron.fetchRoles(idpUrl, true)) as {
        roles?: AwsRole[];
        credentialsRequired?: boolean;
        prefillUsername?: string;
        error?: string;
      };
      if (result.roles) {
        receive(result.roles);
      } else if (result.credentialsRequired) {
        setNeedCreds(true);
        setUsername(result.prefillUsername ?? '');
      } else if (result.error) {
        setError(result.error);
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
      setPhase(null);
    }
  };

  const loadWithCreds = async () => {
    setBusy(true);
    setError(null);
    setPhase('Signing in and looking up your accounts…');
    try {
      const result = (await window.electron.fetchRolesWithCredentials(
        idpUrl,
        username,
        password
      )) as { roles?: AwsRole[]; error?: string };
      if (result.roles) {
        receive(result.roles);
        // Only after a successful sign-in: never persist credentials we know are wrong.
        if (remember && canRemember) {
          const saved = (await window.electron.setDefaultCredentials(username, password)) as
            | void
            | { success: false; error: string };
          if (!saved) setSavedNotice(true);
        }
      } else {
        setError(result.error ?? 'Sign-in failed.');
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
      setPhase(null);
      setPassword('');
    }
  };

  const create = async () => {
    if (!accounts) return;
    setBusy(true);
    setPhase('Creating profiles…');
    try {
      const created = await persistImport({
        accounts,
        selected,
        source: { kind: 'saml', idpEntryUrl: idpUrl.trim() },
        rememberOrgDefaults: true,
      });
      addCreated(created);
      next();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
      setPhase(null);
    }
  };

  return (
    <>
      <StepHeader
        title="Import SAML accounts"
        blurb="Sign in to your identity provider and pick which account and role pairs to create profiles for."
      />

      <div className="max-w-2xl">
        <label className="block text-sm text-discord-textMuted">IdP entry URL</label>
        <div className="mt-1.5 flex gap-2">
          <input
            value={idpUrl}
            onChange={(e) => setIdpUrl(e.target.value)}
            placeholder="https://adfs.example.com/adfs/ls/..."
            className="flex-1 rounded-button border border-discord-border bg-discord-darkest px-3 py-2 text-discord-text placeholder-discord-textMuted focus:border-discord-accent focus:outline-none"
          />
          {!needCreds && (
            <button
              onClick={load}
              disabled={busy || !idpUrl.trim()}
              className="rounded-button border border-discord-border bg-discord-darkest px-4 py-2 text-sm text-discord-textMuted hover:bg-discord-dark hover:text-discord-text disabled:opacity-50 transition-colors"
            >
              {busy ? 'Loading…' : 'Load roles'}
            </button>
          )}
        </div>
      </div>

      {needCreds && (
        <div className="mt-4 max-w-md rounded-card border border-discord-border bg-discord-panel/50 p-4">
          <p className="text-sm text-discord-textMuted">
            Sign in to your identity provider to load your roles.
          </p>
          <input
            value={username}
            onChange={(e) => setUsername(e.target.value)}
            placeholder="Username"
            className="mt-3 w-full rounded-button border border-discord-border bg-discord-darkest px-3 py-2 text-sm text-discord-text placeholder-discord-textMuted focus:border-discord-accent focus:outline-none"
          />
          <input
            type="password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && loadWithCreds()}
            placeholder="Password"
            className="mt-2 w-full rounded-button border border-discord-border bg-discord-darkest px-3 py-2 text-sm text-discord-text placeholder-discord-textMuted focus:border-discord-accent focus:outline-none"
          />
          <label className="mt-3 flex items-start gap-2">
            <input
              type="checkbox"
              checked={remember && canRemember}
              disabled={!canRemember}
              onChange={(e) => setRemember(e.target.checked)}
              className="mt-0.5 rounded border-discord-border text-discord-accent focus:ring-discord-accent disabled:opacity-50"
            />
            <span className="text-xs text-discord-textMuted">
              {canRemember
                ? 'Remember these so profiles refresh in the background without asking again.'
                : 'Set a master password in Settings to save these; otherwise you will be asked each time credentials are needed.'}
            </span>
          </label>
          <button
            onClick={loadWithCreds}
            disabled={busy || !idpUrl.trim() || !username.trim() || !password}
            className="mt-3 inline-flex items-center gap-2 rounded-button bg-discord-accent px-4 py-2 text-sm font-semibold text-white hover:bg-discord-accentHover disabled:opacity-50"
          >
            {busy && (
              <svg className="h-4 w-4 animate-spin" viewBox="0 0 24 24" fill="none" aria-hidden>
                <circle className="opacity-25" cx="12" cy="12" r="10" stroke="currentColor" strokeWidth="4" />
                <path className="opacity-90" fill="currentColor" d="M4 12a8 8 0 018-8v4a4 4 0 00-4 4H4z" />
              </svg>
            )}
            {busy ? 'Signing in…' : 'Sign in & load roles'}
          </button>
        </div>
      )}

      {accounts && accounts.length > 0 && (
        <div className="mt-6 flex min-h-0 flex-1 flex-col">
          <AccountPicker accounts={accounts} selected={selected} onChange={setSelected} />
        </div>
      )}

      {busy && phase && <Spinner label={phase} />}
      {savedNotice && (
        <p className="mt-4 rounded-card border border-discord-border bg-discord-panel/50 px-4 py-3 text-sm text-discord-textMuted">
          Sign-in details saved. Profiles will refresh in the background without asking again.
        </p>
      )}
      <StepError message={error} />
      <StepFooter
        onNext={accounts && accounts.length > 0 ? create : undefined}
        nextLabel={`Create ${selected.size} profile${selected.size === 1 ? '' : 's'}`}
        nextDisabled={selected.size === 0}
        onSkip={skip}
        onBack={back}
        busy={busy}
      />
    </>
  );
}

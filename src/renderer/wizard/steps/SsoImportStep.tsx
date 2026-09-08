import { useEffect, useState } from 'react';
import { AccountPicker } from '../AccountPicker';
import { fromSsoAccounts, type ImportableAccount } from '../importModel';
import { StepError, StepFooter, StepHeader, type StepProps } from '../SetupWizard';
import { persistImport } from '../persistImport';
import { normalizeStartUrl } from '../../../shared/ssoOrg';

/** Bulk import of Identity Center accounts. Needs no master password and no stored credentials. */
export function SsoImportStep({ next, skip, back, addCreated }: StepProps) {
  const [startUrl, setStartUrl] = useState('');
  const [region, setRegion] = useState('');
  const [accounts, setAccounts] = useState<ImportableAccount[] | null>(null);
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [displayNames, setDisplayNames] = useState<Record<string, string>>({});

  useEffect(() => {
    window.electron.getSettings().then((s) => {
      setStartUrl(s?.defaultSsoStartUrl ?? '');
      setRegion(s?.defaultSsoRegion ?? '');
      setDisplayNames(s?.accountDisplayNames ?? {});
    });
  }, []);

  const load = async () => {
    const url = normalizeStartUrl(startUrl);
    if (!url || !region.trim()) {
      setError('Enter the SSO start URL and region.');
      return;
    }
    setBusy(true);
    setError(null);
    try {
      const result = await window.electron.ssoListAccounts(url, region.trim());
      if ('error' in result) {
        setError(result.error);
      } else {
        const mapped = fromSsoAccounts(result.accounts, displayNames);
        setAccounts(mapped);
        if (mapped.length === 0) {
          // Sign-in succeeded but nothing is assigned: almost always "not provisioned yet".
          setError(
            'Signed in successfully, but no AWS accounts are assigned to you yet. Ask whoever manages Identity Center to grant you access, then try again. You can skip this and add them later.'
          );
        }
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  const create = async () => {
    if (!accounts) return;
    setBusy(true);
    try {
      const created = await persistImport({
        accounts,
        selected,
        source: {
          kind: 'identityCenter',
          startUrl: normalizeStartUrl(startUrl),
          region: region.trim(),
        },
        rememberOrgDefaults: true,
      });
      addCreated(created);
      next();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <StepHeader
        title="Import Identity Center accounts"
        blurb="From the AWS access portal: open any account, choose Access keys, and copy the two values from the AWS IAM Identity Center credentials tab."
      />

      <div className="max-w-2xl space-y-3">
        <div>
          <label className="block text-sm text-discord-textMuted">SSO start URL</label>
          <input
            value={startUrl}
            onChange={(e) => setStartUrl(e.target.value)}
            placeholder="https://d-xxxxxxxxxx.awsapps.com/start"
            className="mt-1.5 w-full rounded-button border border-discord-border bg-discord-darkest px-3 py-2 text-discord-text placeholder-discord-textMuted focus:border-discord-accent focus:outline-none"
          />
        </div>
        <div className="flex items-end gap-2">
          <div className="flex-1">
            <label className="block text-sm text-discord-textMuted">SSO region</label>
            <input
              value={region}
              onChange={(e) => setRegion(e.target.value)}
              placeholder="us-west-2"
              className="mt-1.5 w-full rounded-button border border-discord-border bg-discord-darkest px-3 py-2 text-discord-text placeholder-discord-textMuted focus:border-discord-accent focus:outline-none"
            />
          </div>
          <button
            onClick={load}
            disabled={busy || !startUrl.trim() || !region.trim()}
            className="rounded-button border border-discord-border bg-discord-darkest px-4 py-2 text-sm text-discord-textMuted hover:bg-discord-dark hover:text-discord-text disabled:opacity-50 transition-colors"
          >
            {busy ? 'Waiting for browser…' : 'Sign in & load'}
          </button>
        </div>
      </div>

      {accounts && accounts.length > 0 && (
        <div className="mt-6 flex min-h-0 flex-1 flex-col">
          <AccountPicker accounts={accounts} selected={selected} onChange={setSelected} />
        </div>
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

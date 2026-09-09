import { useEffect, useState } from 'react';
import { validateMasterPassword } from '../../../shared/masterPassword';
import { StepError, StepFooter, StepHeader, type StepProps } from '../SetupWizard';

/**
 * Must come before the credentials step: setDefaultCredentials refuses without a master password.
 * Also before the Identity Center import, because createMasterPassword() deletes stored SSO
 * sessions rather than re-encrypting them.
 *
 * Offered for both auth types: it encrypts saved IdP credentials AND stored Identity Center
 * sessions, and it is what makes the app ask to be unlocked after a restart. Still skippable —
 * without it nothing is encrypted at rest beyond the OS keystore and the app never locks.
 */
export function MasterPasswordStep({ patch, next, skip, back }: StepProps) {
  const [password, setPassword] = useState('');
  const [confirm, setConfirm] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [alreadySet, setAlreadySet] = useState(false);

  useEffect(() => {
    window.electron.getMasterPasswordEnabled?.().then((on) => {
      setAlreadySet(!!on);
      if (on) patch({ masterPasswordSet: true });
    });
    // patch is stable enough for a mount-only check; re-running would loop on state updates.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const submit = async () => {
    setError(null);
    const problem = validateMasterPassword(password);
    if (problem) return setError(problem);
    if (password !== confirm) return setError('Passwords do not match.');

    setBusy(true);
    try {
      const result = await window.electron.createMasterPassword(password, confirm);
      if (result.success) {
        patch({ masterPasswordSet: true });
        next();
      } else {
        setError(result.error);
      }
    } finally {
      setBusy(false);
    }
  };

  if (alreadySet) {
    return (
      <>
        <StepHeader
          title="Master password"
          blurb="You already have one set, so there is nothing to do here."
        />
        <StepFooter onNext={next} onBack={back} />
      </>
    );
  }

  return (
    <>
      <StepHeader
        title="Set a master password"
        blurb="Encrypts what this app stores on your machine — saved sign-in details and Identity Center sessions — and locks the app until you enter it after a restart. It is never stored anywhere: if you forget it, you sign in again to rebuild what was saved."
      />
      <div className="max-w-md space-y-4">
        <div>
          <label className="block text-sm text-discord-textMuted">Master password</label>
          <input
            type="password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            className="mt-1.5 w-full rounded-button border border-discord-border bg-discord-darkest px-3 py-2 text-discord-text focus:border-discord-accent focus:outline-none"
          />
        </div>
        <div>
          <label className="block text-sm text-discord-textMuted">Confirm</label>
          <input
            type="password"
            value={confirm}
            onChange={(e) => setConfirm(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && submit()}
            className="mt-1.5 w-full rounded-button border border-discord-border bg-discord-darkest px-3 py-2 text-discord-text focus:border-discord-accent focus:outline-none"
          />
        </div>
      </div>
      <StepError message={error} />
      <StepFooter
        onNext={submit}
        nextLabel="Set password"
        nextDisabled={!password || !confirm}
        onSkip={skip}
        skipLabel="Skip — I only use Identity Center"
        onBack={back}
        busy={busy}
      />
    </>
  );
}

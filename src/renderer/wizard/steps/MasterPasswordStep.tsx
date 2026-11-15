import { useEffect, useId, useState } from 'react';
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
export function MasterPasswordStep({ ctx, patch, next, skip, back }: StepProps) {
  const fid = useId();
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
    if (password !== confirm) return setError('Passwords do not match');

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
        blurb={`Encrypts what this app stores on your machine (saved sign-in details and Identity Center sessions) and locks the app after a restart until you enter it. It is never stored. If you forget it, sign in again to rebuild what was saved.${ctx.wantSaml ? ' Saving your IdP sign-in for automatic refresh needs one.' : ''}`}
      />
      <div className="max-w-md space-y-4">
        <div>
          <label htmlFor={`${fid}-pass`} className="label">Master password</label>
          <input
            id={`${fid}-pass`}
            type="password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            className="input mt-1.5"
          />
        </div>
        <div>
          <label htmlFor={`${fid}-confirm`} className="label">Confirm</label>
          <input
            id={`${fid}-confirm`}
            type="password"
            value={confirm}
            onChange={(e) => setConfirm(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && submit()}
            className="input mt-1.5"
          />
        </div>
      </div>
      <StepError message={error} />
      <StepFooter
        onNext={submit}
        nextLabel="Set password"
        nextDisabled={!password || !confirm}
        onSkip={skip}
        skipLabel={ctx.wantSaml ? 'Skip, ask me to sign in each time' : 'Skip'}
        onBack={back}
        busy={busy}
      />
    </>
  );
}

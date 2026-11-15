import { useId, useState } from 'react';
import { StepError, StepFooter, StepHeader, type StepProps } from '../SetupWizard';

/**
 * Stores the IdP username/password used to fetch SAML roles and refresh SAML profiles silently.
 * Gated on the master password: without one, setDefaultCredentials returns MASTER_PASSWORD_REQUIRED.
 */
export function CredentialsStep({ ctx, patch, next, skip, back }: StepProps) {
  const fid = useId();
  const [username, setUsername] = useState('');
  const [password, setPassword] = useState('');
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  if (!ctx.masterPasswordSet) {
    return (
      <>
        <StepHeader
          title="Sign-in details"
          blurb="Saving your IdP username and password needs a master password to encrypt them, and that step was skipped. You can still add SAML accounts — you will just be asked to sign in each time instead."
        />
        <StepFooter onNext={next} nextLabel="Continue" onBack={back} />
      </>
    );
  }

  const submit = async () => {
    setError(null);
    setBusy(true);
    try {
      const result = (await window.electron.setDefaultCredentials(username, password)) as
        | void
        | { success: false; error: string };
      if (result && 'error' in result) {
        setError('Could not save credentials. Set a master password first.');
        return;
      }
      patch({ credentialsSaved: true });
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
        title="Your IdP sign-in"
        blurb="Used to fetch your list of SAML roles and to refresh those profiles in the background. Stored encrypted on this machine and never sent anywhere except your identity provider."
      />
      <div className="max-w-md space-y-4">
        <div>
          <label htmlFor={`${fid}-user`} className="label">Username</label>
          <input
            id={`${fid}-user`}
            value={username}
            onChange={(e) => setUsername(e.target.value)}
            placeholder="e.g. SA account"
            className="input mt-1.5"
          />
        </div>
        <div>
          <label htmlFor={`${fid}-pass`} className="label">Password</label>
          <input
            id={`${fid}-pass`}
            type="password"
            value={password}
            onChange={(e) => setPassword(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && submit()}
            className="input mt-1.5"
          />
        </div>
      </div>
      <StepError message={error} />
      <StepFooter
        onNext={submit}
        nextLabel="Save"
        nextDisabled={!username.trim() || !password}
        onSkip={skip}
        onBack={back}
        busy={busy}
      />
    </>
  );
}

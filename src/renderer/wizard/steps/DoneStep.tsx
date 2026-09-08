import { StepFooter, StepHeader, type StepProps } from '../SetupWizard';
import { WizardIcon } from '../WizardIcons';
import type { WizardMode } from '../SetupWizard';

/** Summary of what the run actually produced, so "finish" is not a leap of faith. */
export function DoneStep({ ctx, next, back, mode }: StepProps & { mode: WizardMode }) {
  const created = ctx.created;
  const samlCount = created.filter((p) => (p.authType ?? 'saml') === 'saml').length;
  const ssoCount = created.length - samlCount;

  const lines: string[] = [];
  if (samlCount) lines.push(`${samlCount} SAML profile${samlCount === 1 ? '' : 's'}`);
  if (ssoCount) lines.push(`${ssoCount} Identity Center profile${ssoCount === 1 ? '' : 's'}`);
  if (ctx.masterPasswordSet) lines.push('Master password set');
  if (ctx.credentialsSaved) lines.push('IdP sign-in saved');

  return (
    <>
      <StepHeader
        title={created.length > 0 ? 'All set' : 'Nothing added yet'}
        blurb={
          created.length > 0
            ? 'Credentials refresh automatically in the background. Use the cloud button on any profile to open the AWS console for it.'
            : 'You can add accounts any time from the Add profile menu on the Profiles page.'
        }
      />

      {lines.length > 0 && (
        <ul className="max-w-md space-y-2">
          {lines.map((l) => (
            <li
              key={l}
              className="flex items-center gap-3 rounded-card border border-discord-border bg-discord-panel/50 px-4 py-3 text-sm text-discord-text"
            >
              <span className="text-discord-accent">
                <WizardIcon name="check" className="h-4 w-4" />
              </span>
              {l}
            </li>
          ))}
        </ul>
      )}

      {created.length > 0 && (
        <ul className="mt-4 max-w-md space-y-1">
          {created.slice(0, 8).map((p) => (
            <li key={p.id} className="truncate text-xs text-discord-textMuted">
              {p.name} <span className="opacity-60">→ --profile {p.credentialProfileName}</span>
            </li>
          ))}
          {created.length > 8 && (
            <li className="text-xs text-discord-textMuted">…and {created.length - 8} more</li>
          )}
        </ul>
      )}

      <StepFooter
        onNext={next}
        nextLabel={mode === 'firstRun' ? 'Finish' : 'Done'}
        onBack={back}
      />
    </>
  );
}

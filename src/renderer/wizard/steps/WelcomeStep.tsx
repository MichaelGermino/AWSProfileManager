import { StepFooter, StepHeader, type StepProps } from '../SetupWizard';
import { WizardIcon } from '../WizardIcons';

const POINTS: { icon: 'key' | 'cloud' | 'chat'; title: string; body: string }[] = [
  {
    icon: 'key',
    title: 'Short-lived credentials, refreshed for you',
    body: 'Profiles write temporary keys to ~/.aws/credentials so the AWS CLI and your local apps just work.',
  },
  {
    icon: 'cloud',
    title: 'SAML and IAM Identity Center side by side',
    body: 'Import whole sets of accounts at once, from either sign-in method or both.',
  },
  {
    icon: 'chat',
    title: 'A terminal and console built in',
    body: 'Open the AWS console for any profile in one click, without signing in again.',
  },
];

export function WelcomeStep({ next, exit }: StepProps) {
  return (
    <>
      <StepHeader
        title="Welcome to AWS Profile Manager"
        blurb="A few minutes now and your accounts are ready to use. Every step can be skipped, and nothing here is permanent — it is all editable later in Settings."
      />
      <div className="space-y-4">
        {POINTS.map((p) => (
          <div key={p.title} className="flex gap-4 rounded-card border border-discord-border bg-discord-panel/50 p-4">
            <span className="mt-0.5 text-discord-accent">
              <WizardIcon name={p.icon} />
            </span>
            <div>
              <div className="text-sm font-medium text-discord-text">{p.title}</div>
              <div className="mt-0.5 text-sm text-discord-textMuted">{p.body}</div>
            </div>
          </div>
        ))}
      </div>
      <StepFooter onNext={next} nextLabel="Get started" onSkip={exit} skipLabel="Skip setup" />
    </>
  );
}

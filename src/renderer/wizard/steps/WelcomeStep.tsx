import { StepFooter, StepHeader, type StepProps } from '../SetupWizard';
import { WizardIcon } from '../WizardIcons';

const POINTS: { icon: 'key' | 'cloud' | 'chat'; title: string; body: string }[] = [
  {
    icon: 'key',
    title: 'Short-lived credentials, refreshed for you',
    body: 'Profiles write temporary keys to ~/.aws/credentials, so the AWS CLI and SDKs pick them up.',
  },
  {
    icon: 'cloud',
    title: 'SAML and IAM Identity Center side by side',
    body: 'Import whole sets of accounts at once, from either sign-in method or both.',
  },
  {
    icon: 'chat',
    title: 'Console and terminal for each profile',
    body: 'Open the AWS console in one click, or a shell with the profile already set.',
  },
];

export function WelcomeStep({ next, exit }: StepProps) {
  return (
    <>
      <StepHeader
        title="Welcome to AWS Profile Manager"
        blurb="Set up your accounts in a few minutes. Every step can be skipped and changed later in Settings."
      />
      <ul className="divide-y divide-ui-border border-y border-ui-border">
        {POINTS.map((p) => (
          <li key={p.title} className="flex gap-3 py-4">
            <span className="mt-0.5 text-ui-textMuted">
              <WizardIcon name={p.icon} />
            </span>
            <div>
              <div className="text-sm font-medium text-ui-text">{p.title}</div>
              <div className="mt-0.5 text-sm text-ui-textMuted">{p.body}</div>
            </div>
          </li>
        ))}
      </ul>
      <StepFooter onNext={next} nextLabel="Get started" onSkip={exit} skipLabel="Skip setup" />
    </>
  );
}

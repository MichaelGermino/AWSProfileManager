import { StepFooter, StepHeader, type StepProps } from '../SetupWizard';
import { WizardIcon } from '../WizardIcons';

/**
 * Which importers to run. Both can be chosen; the wizard then shows both steps in turn, because
 * the two need different credentials and cannot share one picker.
 */
export function ChooseTypeStep({ ctx, patch, next, skip }: StepProps) {
  const options = [
    {
      key: 'saml' as const,
      icon: 'building' as const,
      title: 'SAML / ADFS',
      body: 'Sign in with your IdP username and password. Refreshes silently in the background.',
      on: ctx.wantSaml,
      toggle: () => patch({ wantSaml: !ctx.wantSaml }),
    },
    {
      key: 'sso' as const,
      icon: 'identity' as const,
      title: 'IAM Identity Center',
      body: 'Sign in once in a browser. Renews silently until the SSO session expires.',
      on: ctx.wantSso,
      toggle: () => patch({ wantSso: !ctx.wantSso }),
    },
  ];

  return (
    <>
      <StepHeader
        title="How do you sign in to AWS?"
        blurb="Pick either or both. Not sure? Your AWS access portal URL looks like d-xxxxxxxxxx.awsapps.com for Identity Center; anything else is probably SAML."
      />
      <div className="grid gap-4 sm:grid-cols-2">
        {options.map((o) => (
          <button
            key={o.key}
            type="button"
            onClick={o.toggle}
            aria-pressed={o.on}
            className={`rounded-card border p-5 text-left transition-colors ${
 o.on
 ? 'border-ui-accent bg-ui-accent/5'
 : 'border-ui-border bg-ui-panel/50 hover:border-ui-textMuted'
 }`}
          >
            <span className={o.on ? 'text-ui-accent' : 'text-ui-textMuted'}>
              <WizardIcon name={o.icon} className="h-7 w-7" />
            </span>
            <div className="mt-3 text-sm font-medium text-ui-text">{o.title}</div>
            <div className="mt-1 text-sm text-ui-textMuted">{o.body}</div>
          </button>
        ))}
      </div>
      <StepFooter
        onNext={next}
        nextDisabled={!ctx.wantSaml && !ctx.wantSso}
        onSkip={skip}
        skipLabel="I'll add accounts later"
      />
    </>
  );
}

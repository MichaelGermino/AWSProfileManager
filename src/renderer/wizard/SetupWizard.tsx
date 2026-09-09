import { useEffect, useMemo, useState } from 'react';
import type { Profile } from '../../shared/types';
import { WizardIcon, type WizardIconName } from './WizardIcons';
import { WelcomeStep } from './steps/WelcomeStep';
import { OrgConfigStep } from './steps/OrgConfigStep';
import { MasterPasswordStep } from './steps/MasterPasswordStep';
import { CredentialsStep } from './steps/CredentialsStep';
import { ChooseTypeStep } from './steps/ChooseTypeStep';
import { SamlImportStep } from './steps/SamlImportStep';
import { SsoImportStep } from './steps/SsoImportStep';
import { AiStep } from './steps/AiStep';
import { DoneStep } from './steps/DoneStep';

/**
 * Setup wizard, used two ways:
 *   - first run, with every step
 *   - "Add accounts…", with only the import steps
 *
 * Step order is not cosmetic. `setDefaultCredentials` refuses without a master password, and
 * `fetchRolesForIdp` needs stored credentials, so master password → credentials → SAML import is a
 * real dependency chain. Identity Center has no such chain, so those steps are shown only when the
 * user says they use SAML — which is why the type question has to come before them.
 */

export type StepKey =
  | 'welcome'
  | 'orgConfig'
  | 'masterPassword'
  | 'credentials'
  | 'chooseType'
  | 'samlImport'
  | 'ssoImport'
  | 'ai'
  | 'done';

export type WizardMode = 'firstRun' | 'import';

/**
 * The type question comes FIRST, because everything after it is conditional on the answer.
 *
 * `credentials` exists only to make SAML work — an Identity Center user has no IdP password to
 * store. The master password is NOT SAML-only: it also encrypts stored Identity Center sessions
 * and is what locks the app on restart, so it is offered for either type.
 *
 * masterPassword must stay ahead of ssoImport: createMasterPassword() deletes stored SSO sessions
 * rather than re-encrypting them, so setting one after signing in would silently throw that
 * session away and demand another browser round-trip.
 */
const FIRST_RUN_STEPS: StepKey[] = [
  'welcome',
  // Before anything it could prefill.
  'orgConfig',
  'chooseType',
  'masterPassword',
  'credentials',
  'samlImport',
  'ssoImport',
  'ai',
  'done',
];

// 'orgConfig' leads here too: skipping first-run setup was previously a one-way door, with no
// route back to the file that seeds account display names. It filters itself out once one has
// been applied, so returning users are not asked again.
const IMPORT_STEPS: StepKey[] = ['orgConfig', 'chooseType', 'masterPassword', 'credentials', 'samlImport', 'ssoImport', 'done'];

const STEP_META: Record<StepKey, { title: string; icon: WizardIconName }> = {
  welcome: { title: 'Welcome', icon: 'sparkles' },
  orgConfig: { title: 'Organization file', icon: 'building' },
  masterPassword: { title: 'Master password', icon: 'lock' },
  credentials: { title: 'Sign-in details', icon: 'key' },
  chooseType: { title: 'Account type', icon: 'cloud' },
  samlImport: { title: 'SAML accounts', icon: 'building' },
  ssoImport: { title: 'Identity Center', icon: 'identity' },
  ai: { title: 'AI assistant', icon: 'chat' },
  done: { title: 'Finish', icon: 'check' },
};

/**
 * Which steps apply, given what the user has chosen and what is already configured.
 *
 * Exported so the step sequence can be walked in a test: this filter and the cursor that moves
 * through it have now produced two navigation bugs, and reasoning about them by hand is what let
 * both through.
 */
export function isStepVisible(step: StepKey, ctx: WizardContext): boolean {
  if (step === 'orgConfig') return !ctx.orgConfigAnswered;
  if (step === 'samlImport') return ctx.wantSaml;
  if (step === 'ssoImport') return ctx.wantSso;
  // Not SAML-only: the master password also encrypts Identity Center sessions and locks the app.
  if (step === 'masterPassword') return (ctx.wantSaml || ctx.wantSso) && !ctx.masterPasswordSet;
  // SAML-only: an Identity Center user has no IdP password to store.
  if (step === 'credentials') return ctx.wantSaml && !ctx.credentialsSaved;
  return true;
}

/** First still-visible step after `key`, in wizard order. Null when `key` is the last one. */
export function stepAfterIn(order: StepKey[], key: StepKey, visible: StepKey[]): StepKey | null {
  for (let i = order.indexOf(key) + 1; i < order.length; i++) {
    if (visible.includes(order[i])) return order[i];
  }
  return null;
}

/** Mirror of stepAfterIn, for Back. */
export function stepBeforeIn(order: StepKey[], key: StepKey, visible: StepKey[]): StepKey | null {
  for (let i = order.indexOf(key) - 1; i >= 0; i--) {
    if (visible.includes(order[i])) return order[i];
  }
  return null;
}

export const WIZARD_STEP_ORDER = { firstRun: FIRST_RUN_STEPS, import: IMPORT_STEPS };

export interface WizardContext {
  /** Which import steps the user opted into on the chooseType step. */
  wantSaml: boolean;
  wantSso: boolean;
  /** Profiles created during this run, for the summary. */
  created: Profile[];
  masterPasswordSet: boolean;
  credentialsSaved: boolean;
  /** The org-config question has been answered before — imported OR declined — so don't ask again.
   *  Settings → Organization configuration → Import… is the way back either way. */
  orgConfigAnswered: boolean;
}

export function SetupWizard({
  mode,
  onClose,
}: {
  mode: WizardMode;
  onClose: (createdAny: boolean) => void;
}) {
  const allSteps = mode === 'firstRun' ? FIRST_RUN_STEPS : IMPORT_STEPS;
  const [currentKey, setCurrentKey] = useState<StepKey>(allSteps[0]);
  /** Step we are advancing away from; resolved to a destination after ctx settles. */
  const [advanceFrom, setAdvanceFrom] = useState<StepKey | null>(null);
  const [direction, setDirection] = useState<'forward' | 'back'>('forward');
  const [skipped, setSkipped] = useState<Set<StepKey>>(new Set());
  const [ctx, setCtx] = useState<WizardContext>({
    wantSaml: false,
    wantSso: false,
    created: [],
    masterPasswordSet: false,
    credentialsSaved: false,
    orgConfigAnswered: false,
  });

  /**
   * Seed from what is actually configured, so the master-password and credentials steps are
   * skipped when they are already done. Without this the flags start false and a returning user
   * would be walked through setup they completed long ago.
   */
  useEffect(() => {
    let cancelled = false;
    void (async () => {
      const [mpEnabled, creds, settings] = await Promise.all([
        window.electron.getMasterPasswordEnabled(),
        window.electron.getDefaultCredentialsDisplay(),
        window.electron.getSettings(),
      ]);
      if (cancelled) return;
      setCtx((c) => ({
        ...c,
        masterPasswordSet: !!mpEnabled,
        credentialsSaved: !!creds?.hasPassword,
        orgConfigAnswered: !!(settings?.orgConfigImportedAt || settings?.orgConfigDeclinedAt),
      }));
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  /**
   * Steps are filtered by the type choice, which is made mid-wizard, so this recomputes as the
   * user toggles. `credentials` rides on wantSaml alone — it is a prerequisite for fetching SAML
   * roles and refreshing SAML profiles, and does nothing for Identity Center. The master password
   * applies to both: it encrypts SAML credentials AND Identity Center sessions, and gates the app
   * on restart either way.
   *
   * On the import entry the credential steps only appear when they are actually missing — someone
   * adding accounts later has usually set them up already.
   */
  const steps = useMemo(
    () => allSteps.filter((s) => isStepVisible(s, ctx)),
    [allSteps, ctx.wantSaml, ctx.wantSso, ctx.masterPasswordSet, ctx.credentialsSaved, ctx.orgConfigAnswered]
  );

  /**
   * Navigation tracks the step KEY, never its position.
   *
   * `steps` is derived from ctx, so a step that patches ctx on its way out removes ITSELF from the
   * list in the same update — masterPassword sets masterPasswordSet, which is exactly the condition
   * hiding it. A positional cursor then advances one step too far, because the list shrank beneath
   * it: at masterPassword the entry under index 3 became ssoImport, so `index + 1` landed on `ai`
   * and the Identity Center import was skipped entirely.
   *
   * Resolving against allSteps rather than the filtered list also means a stale `steps` is
   * harmless: we search forward from where we are for the first step still visible.
   */
  const stepAfter = (key: StepKey, visible: StepKey[]) => stepAfterIn(allSteps, key, visible);
  const stepBefore = (key: StepKey, visible: StepKey[]) => stepBeforeIn(allSteps, key, visible);

  // If the step we are on filtered itself out, fall FORWARD to the next one rather than snapping
  // back to the start of the wizard.
  const current =
    steps.includes(currentKey) ? currentKey : (stepAfter(currentKey, steps) ?? steps[steps.length - 1] ?? allSteps[0]);
  const index = steps.indexOf(current);

  const goTo = (key: StepKey | null, dir: 'forward' | 'back') => {
    if (!key) return;
    setDirection(dir);
    setCurrentKey(key);
  };

  /**
   * Advancing is deferred by one render on purpose.
   *
   * Steps patch ctx on their way out, and that patch changes which steps exist: chooseType ADDS
   * the import steps, masterPassword REMOVES itself. Resolving the destination inside the click
   * handler uses the pre-patch list and gets both wrong in opposite directions — skipping straight
   * to `ai` after choosing a type, or jumping a step after setting a master password.
   *
   * So record only where we are leaving FROM, and let the effect below pick the target once React
   * has applied the patch and `steps` has been recomputed.
   */
  const next = () => setAdvanceFrom(current);

  useEffect(() => {
    if (!advanceFrom) return;
    setAdvanceFrom(null);
    const target = stepAfterIn(allSteps, advanceFrom, steps);
    if (!target) {
      onClose(ctx.created.length > 0);
      return;
    }
    setDirection('forward');
    setCurrentKey(target);
    // ctx/onClose deliberately omitted: this must run when the recomputed step list lands.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [advanceFrom, steps, allSteps]);

  const back = () => goTo(stepBefore(current, steps), 'back');

  const skip = () => {
    setSkipped((s) => new Set(s).add(current));
    next();
  };

  const patch = (p: Partial<WizardContext>) => setCtx((c) => ({ ...c, ...p }));
  const addCreated = (profiles: Profile[]) =>
    setCtx((c) => ({ ...c, created: [...c.created, ...profiles] }));

  /**
   * Leave the wizard entirely, as distinct from `skip`, which only advances past the current step.
   * The welcome step's "Skip setup" means the former: it landed the user on the org-config screen,
   * which is skipping *into* setup rather than out of it.
   */
  const exit = () => onClose(ctx.created.length > 0);

  const stepProps = { ctx, patch, addCreated, next, skip, back, exit };

  return (
    <div className="fixed inset-0 z-[60] flex bg-discord-darkest">
      {/* Progress rail */}
      <aside className="hidden w-64 shrink-0 flex-col border-r border-discord-border bg-discord-panel p-6 sm:flex">
        <h2 className="text-lg font-bold text-discord-text">
          {mode === 'firstRun' ? 'Set up' : 'Add accounts'}
        </h2>
        <ol className="mt-6 space-y-1">
          {steps.map((s, i) => {
            const done = i < index;
            const active = i === index;
            const wasSkipped = skipped.has(s);
            return (
              <li
                key={s}
                className={`flex items-center gap-3 rounded-button px-3 py-2 text-sm transition-colors ${
                  active
                    ? 'bg-discord-accent/10 text-discord-text'
                    : done
                      ? 'text-discord-textMuted'
                      : 'text-discord-textMuted/60'
                }`}
              >
                <span
                  className={`flex h-6 w-6 shrink-0 items-center justify-center rounded-full border ${
                    active
                      ? 'border-discord-accent text-discord-accent'
                      : done
                        ? 'border-transparent bg-discord-accent/20 text-discord-accent'
                        : 'border-discord-border'
                  }`}
                >
                  <WizardIcon
                    name={done ? (wasSkipped ? 'dash' : 'check') : STEP_META[s].icon}
                    className="h-3.5 w-3.5"
                  />
                </span>
                <span className="truncate">{STEP_META[s].title}</span>
              </li>
            );
          })}
        </ol>
        <div className="mt-auto pt-6">
          <button
            onClick={() => onClose(ctx.created.length > 0)}
            className="text-xs text-discord-textMuted hover:text-discord-text"
          >
            {mode === 'firstRun' ? 'Skip setup' : 'Close'}
          </button>
        </div>
      </aside>

      {/* Step body. key on the step drives the enter animation; reduced-motion disables it. */}
      <main className="flex min-w-0 flex-1 flex-col">
        <div key={current} className={`flex min-h-0 flex-1 flex-col p-8 ${direction === 'forward' ? 'wizard-enter' : 'wizard-enter-back'}`}>
          {current === 'welcome' && <WelcomeStep {...stepProps} />}
          {current === 'orgConfig' && <OrgConfigStep {...stepProps} />}
          {current === 'masterPassword' && <MasterPasswordStep {...stepProps} />}
          {current === 'credentials' && <CredentialsStep {...stepProps} />}
          {current === 'chooseType' && <ChooseTypeStep {...stepProps} />}
          {current === 'samlImport' && <SamlImportStep {...stepProps} />}
          {current === 'ssoImport' && <SsoImportStep {...stepProps} />}
          {current === 'ai' && <AiStep {...stepProps} />}
          {current === 'done' && <DoneStep {...stepProps} mode={mode} />}
        </div>
      </main>
    </div>
  );
}

export interface StepProps {
  ctx: WizardContext;
  patch: (p: Partial<WizardContext>) => void;
  addCreated: (profiles: Profile[]) => void;
  next: () => void;
  /** Advance past this step. */
  skip: () => void;
  back: () => void;
  /** Close the wizard outright. */
  exit: () => void;
}

/** Shared footer so every step's buttons sit in the same place. */
export function StepFooter({
  onNext,
  nextLabel = 'Continue',
  nextDisabled,
  onSkip,
  skipLabel = 'Skip',
  onBack,
  busy,
}: {
  onNext?: () => void;
  nextLabel?: string;
  nextDisabled?: boolean;
  onSkip?: () => void;
  skipLabel?: string;
  onBack?: () => void;
  busy?: boolean;
}) {
  return (
    <div className="mt-6 flex items-center gap-3 border-t border-discord-border pt-5">
      {onNext && (
        <button
          onClick={onNext}
          disabled={nextDisabled || busy}
          className="rounded-button bg-discord-accent px-5 py-2.5 text-sm font-semibold text-white hover:bg-discord-accentHover disabled:cursor-not-allowed disabled:opacity-50 transition-all"
        >
          {busy ? 'Working…' : nextLabel}
        </button>
      )}
      {onSkip && (
        <button
          onClick={onSkip}
          disabled={busy}
          className="rounded-button border border-discord-border px-5 py-2.5 text-sm text-discord-textMuted hover:text-discord-text disabled:opacity-50 transition-colors"
        >
          {skipLabel}
        </button>
      )}
      {onBack && (
        <button
          onClick={onBack}
          disabled={busy}
          className="ml-auto text-sm text-discord-textMuted hover:text-discord-text disabled:opacity-50"
        >
          Back
        </button>
      )}
    </div>
  );
}

/** Consistent heading block for each step. */
export function StepHeader({ title, blurb }: { title: string; blurb?: string }) {
  return (
    <header className="mb-6">
      <h3 className="text-2xl font-bold text-discord-text tracking-tight">{title}</h3>
      {blurb && <p className="mt-1.5 max-w-2xl text-sm text-discord-textMuted">{blurb}</p>}
    </header>
  );
}

/** Inline error, shown in the same spot on every step. */
export function StepError({ message }: { message: string | null }) {
  if (!message) return null;
  return (
    <p className="mt-4 rounded-card border border-discord-danger/40 bg-discord-danger/10 px-4 py-3 text-sm text-discord-danger">
      {message}
    </p>
  );
}

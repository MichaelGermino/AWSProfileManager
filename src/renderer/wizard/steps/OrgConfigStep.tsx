import { useState } from 'react';
import {
  applyOrgConfigToSettings,
  describeOrgConfig,
  isOrgConfigEmpty,
  parseOrgConfig,
  type OrgConfig,
} from '../../../shared/orgConfig';
import { StepError, StepFooter, StepHeader, type StepProps } from '../SetupWizard';
import { WizardIcon } from '../WizardIcons';

/**
 * Optional first step: load a configuration file an admin exported, so the rest of the wizard comes
 * prefilled instead of the user hunting for URLs and account numbers.
 *
 * Applied fill-if-empty. On a fresh install everything is empty so the file wins throughout; for
 * someone re-running setup it will not overwrite choices they already made. Account names merge,
 * with existing entries kept — a hand-edited name should survive importing the org file.
 */
export function OrgConfigStep({ next, skip }: StepProps) {
  const [config, setConfig] = useState<OrgConfig | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [applied, setApplied] = useState(false);

  const choose = async () => {
    setBusy(true);
    setError(null);
    try {
      const result = await window.electron.importOrgConfig();
      if ('canceled' in result) return;
      if (!result.success) {
        setError(result.error);
        return;
      }
      // Re-parse in the renderer: the object crossed IPC, so validate it here too rather than
      // trusting its shape.
      const parsed = parseOrgConfig(result.config);
      if (!parsed || isOrgConfigEmpty(parsed)) {
        setError('That file did not contain any settings this app can use.');
        return;
      }
      setConfig(parsed);
      setApplied(false);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  const apply = async () => {
    if (!config) return;
    setBusy(true);
    setError(null);
    try {
      const settings = await window.electron.getSettings();
      await window.electron.saveSettings(applyOrgConfigToSettings(settings, config));
      setApplied(true);
      next();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  /**
   * Record the "no" as well as the "yes". The wizard filters this step on whether the question has
   * been answered at all — tracking only successful imports meant declining was forgotten and every
   * later bulk import asked again.
   *
   * Recorded even when a file has been chosen but not applied: the user has still answered for now,
   * and Settings → Organization configuration → Import… remains available whenever they change
   * their mind.
   */
  const decline = async () => {
    try {
      const settings = await window.electron.getSettings();
      await window.electron.saveSettings({
        ...settings,
        orgConfigDeclinedAt: new Date().toISOString(),
      });
    } catch {
      // Never block leaving the step on a settings write; worst case it asks once more.
    }
    skip();
  };

  return (
    <>
      <StepHeader
        title="Do you have an organization configuration?"
        blurb="If someone on your team shared a configuration file, load it here and the rest of setup will be filled in for you. It contains URLs and account names only — never passwords or keys."
      />

      <div className="max-w-2xl">
        <button
          onClick={choose}
          disabled={busy}
          className="flex w-full items-center gap-4 rounded-card border border-dashed border-discord-border bg-discord-panel/50 p-6 text-left transition-colors hover:border-discord-accent disabled:opacity-50"
        >
          <span className="text-discord-accent">
            <WizardIcon name="sparkles" className="h-7 w-7" />
          </span>
          <span>
            <span className="block text-sm font-medium text-discord-text">
              {config ? 'Choose a different file…' : 'Choose a configuration file…'}
            </span>
            <span className="block text-sm text-discord-textMuted">
              A .json file exported from this app by someone in your organization.
            </span>
          </span>
        </button>

        {/* Without this, a user with no file has no idea one can exist or how to obtain it. */}
        <p className="mt-3 text-xs text-discord-textMuted">
          Don’t have one? Ask a teammate who already uses this app to open{' '}
          <span className="text-discord-text">Settings → General → Organization configuration</span>{' '}
          and click <span className="text-discord-text">Export…</span>, then send you the file. It
          holds only URLs and account names — no passwords, keys or credentials — so it is safe to
          share. You can also skip this and enter the details yourself.
        </p>

        {config && (
          <div className="mt-4 rounded-card border border-discord-border bg-discord-panel/50 p-4">
            <div className="text-sm font-medium text-discord-text">
              {config.organizationName ?? 'Organization configuration'}
            </div>
            <p className="mt-1 text-xs text-discord-textMuted">This file provides:</p>
            <ul className="mt-2 space-y-1">
              {describeOrgConfig(config).map((line) => (
                <li key={line} className="flex items-center gap-2 text-sm text-discord-textMuted">
                  <span className="text-discord-accent">
                    <WizardIcon name="check" className="h-3.5 w-3.5" />
                  </span>
                  {line}
                </li>
              ))}
            </ul>
            {applied && (
              <p className="mt-3 text-xs text-discord-textMuted">Applied to your settings.</p>
            )}
          </div>
        )}
      </div>

      <StepError message={error} />
      <StepFooter
        onNext={config ? apply : undefined}
        nextLabel="Use this configuration"
        onSkip={decline}
        skipLabel={config ? 'Skip' : "I don't have one"}
        busy={busy}
      />
    </>
  );
}

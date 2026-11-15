import { useEffect, useId, useState } from 'react';
import { StepError, StepFooter, StepHeader, type StepProps } from '../SetupWizard';

/**
 * Optional Open WebUI configuration for the terminal's AI assistant. Deliberately last and clearly
 * optional — most users will skip it, and nothing else depends on it.
 *
 * The model id is instance-specific, so there is no sensible default; the list is fetched once the
 * URL and key are saved.
 */
export function AiStep({ next, skip, back }: StepProps) {
  const fid = useId();
  const [apiUrl, setApiUrl] = useState('');
  const [apiKey, setApiKey] = useState('');
  /** A key is already saved in main (the window never receives it). */
  const [keySaved, setKeySaved] = useState(false);
  const [model, setModel] = useState('');
  const [models, setModels] = useState<string[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    window.electron.getSettings().then((s) => {
      setApiUrl(s?.openWebUiApiUrl ?? '');
      setKeySaved(s?.openWebUiApiKeySet === true);
      setModel(s?.openWebUiModel ?? '');
    });
  }, []);

  /** Saving first is required: the models call reads the URL and key from settings in main. */
  const saveAndLoadModels = async () => {
    setBusy(true);
    setError(null);
    try {
      const settings = await window.electron.getSettings();
      await window.electron.saveSettings({
        ...settings,
        openWebUiApiUrl: apiUrl.trim(),
        // Empty field = keep the saved key.
        ...(apiKey.trim() ? { openWebUiApiKey: apiKey.trim() } : {}),
      });
      const result = await window.electron.getAiModels();
      if ('error' in result) setError(result.error);
      else {
        setModels(result.models);
        if (result.models.length === 0) setError('The instance returned no models.');
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setBusy(false);
    }
  };

  const saveModel = async () => {
    setBusy(true);
    try {
      const settings = await window.electron.getSettings();
      await window.electron.saveSettings({ ...settings, openWebUiModel: model });
      next();
    } finally {
      setBusy(false);
    }
  };

  return (
    <>
      <StepHeader
        title="AI assistant (optional)"
        blurb="Connect an Open WebUI instance to get AWS CLI help inside the terminal. Skip this if you do not have one — everything else works without it."
      />

      <div className="max-w-2xl space-y-3">
        <div>
          <label htmlFor={`${fid}-url`} className="block text-sm text-discord-textMuted">API URL</label>
          <input
            id={`${fid}-url`}
            value={apiUrl}
            onChange={(e) => setApiUrl(e.target.value)}
            placeholder="https://your-instance.example.com/api"
            className="mt-1.5 w-full rounded-button border border-discord-border bg-discord-darkest px-3 py-2 text-discord-text placeholder-discord-textMuted focus:border-discord-accent focus:outline-none"
          />
        </div>
        <div>
          <label htmlFor={`${fid}-key`} className="block text-sm text-discord-textMuted">API key</label>
          <input
            id={`${fid}-key`}
            type="password"
            value={apiKey}
            onChange={(e) => setApiKey(e.target.value)}
            placeholder={keySaved ? 'Saved. Type a new key to replace it.' : undefined}
            className="mt-1.5 w-full rounded-button border border-discord-border bg-discord-darkest px-3 py-2 text-discord-text placeholder-discord-textMuted focus:border-discord-accent focus:outline-none"
          />
        </div>
        <button
          onClick={saveAndLoadModels}
          disabled={busy || !apiUrl.trim() || (!apiKey.trim() && !keySaved)}
          className="rounded-button border border-discord-border bg-discord-darkest px-4 py-2 text-sm text-discord-textMuted hover:bg-discord-dark hover:text-discord-text disabled:opacity-50 transition-colors"
        >
          {busy ? 'Checking…' : 'Save & load models'}
        </button>

        {models.length > 0 && (
          <div>
            <label htmlFor={`${fid}-model`} className="block text-sm text-discord-textMuted">Model</label>
            <select
              id={`${fid}-model`}
              value={model}
              onChange={(e) => setModel(e.target.value)}
              className="mt-1.5 w-full rounded-button border border-discord-border bg-discord-darkest px-3 py-2 text-discord-text focus:border-discord-accent focus:outline-none"
            >
              <option value="">Select a model…</option>
              {models.map((m) => (
                <option key={m} value={m}>
                  {m}
                </option>
              ))}
            </select>
          </div>
        )}
      </div>

      <StepError message={error} />
      <StepFooter
        onNext={models.length > 0 ? saveModel : undefined}
        nextLabel="Save"
        nextDisabled={!model}
        onSkip={skip}
        onBack={back}
        busy={busy}
      />
    </>
  );
}

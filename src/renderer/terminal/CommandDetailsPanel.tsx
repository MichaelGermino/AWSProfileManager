/**
 * Command Details panel: name, description, syntax, options, examples.
 * Each example has an "Insert Into Terminal" button.
 * Shows "Read more" link to AWS CLI Docs and "Ask AI how to use this" when available.
 */

import type { AwsCliCommand } from './awsCliMockData';

const AWS_CLI_REFERENCE_BASE = 'https://docs.aws.amazon.com/cli/latest/reference';

/** Derive doc URL from command id (e.g. s3-mb -> .../s3/mb.html). */
function getCommandDocUrl(command: AwsCliCommand): string {
  if (command.docUrl) return command.docUrl;
  const dash = command.id.indexOf('-');
  if (dash === -1) return `${AWS_CLI_REFERENCE_BASE}/${command.id}/`;
  const service = command.id.slice(0, dash);
  const cmd = command.id.slice(dash + 1);
  return `${AWS_CLI_REFERENCE_BASE}/${service}/${cmd}.html`;
}

interface CommandDetailsPanelProps {
  command: AwsCliCommand | null;
  onInsertCommand: (command: string) => void;
  /** Ask the AI how to use the selected command; prompt is sent to the AI Assistant. */
  onAskAI?: (prompt: string) => void;
}

declare global {
  interface Window {
    electron?: { openExternal?: (url: string) => Promise<void> };
  }
}

export function CommandDetailsPanel({ command, onInsertCommand, onAskAI }: CommandDetailsPanelProps) {
  if (!command) {
    return (
      <div className="p-4 text-discord-textMuted text-sm">
        Select a command from the explorer to view details and examples.
      </div>
    );
  }

  const handleAskAI = () => {
    const prompt = `How do I use the AWS CLI command: ${command.syntax}? Explain and give examples.`;
    onAskAI?.(prompt);
  };

  const docUrl = getCommandDocUrl(command);

  return (
    <div className="p-4 overflow-auto h-full">
      <div className="mb-4">
        <div className="flex flex-wrap items-center gap-2 mb-1">
          <h3 className="text-discord-text font-semibold font-mono text-base">{command.name}</h3>
          {onAskAI && (
            <button
              type="button"
              onClick={handleAskAI}
              className="inline-flex items-center gap-1.5 px-2.5 py-1 rounded-lg text-xs font-medium bg-discord-panel text-discord-text border border-discord-border hover:bg-discord-accent/20 hover:border-discord-accent/50 transition-colors"
            >
              <svg className="h-3.5 w-3.5 text-discord-accent" fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden>
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M5 3v4M3 5h4M6 17v4m-2-2h4m5-16l2.286 6.857L21 12l-5.714 2.143L13 21l-2.286-6.857L5 12l5.714-2.143L13 3z" />
              </svg>
              Ask AI how to use this
            </button>
          )}
        </div>
        <p className="text-discord-textMuted text-sm">{command.description}</p>
        <p className="mt-2">
          <a
            href={docUrl}
            target="_blank"
            rel="noopener noreferrer"
            onClick={(e) => {
              e.preventDefault();
              window.electron?.openExternal?.(docUrl);
            }}
            className="text-discord-accent hover:underline text-sm"
          >
            Read more about this command in the AWS CLI Docs →
          </a>
        </p>
      </div>

      <div className="mb-4">
        <h4 className="text-discord-textMuted text-xs font-semibold uppercase tracking-wide mb-1">Syntax</h4>
        <pre className="p-2 rounded bg-discord-darker border border-discord-border text-discord-text text-sm font-mono overflow-x-auto">
          {command.syntax}
        </pre>
      </div>

      {command.options.length > 0 && (
        <div className="mb-4">
          <h4 className="text-discord-textMuted text-xs font-semibold uppercase tracking-wide mb-1">Options</h4>
          <ul className="space-y-1 text-sm">
            {command.options.map((opt) => (
              <li key={opt.name} className="text-discord-text">
                <code className="text-discord-accent font-mono">{opt.name}</code>
                {opt.required && <span className="text-discord-danger text-xs ml-1">required</span>}
                <span className="text-discord-textMuted"> — {opt.description}</span>
              </li>
            ))}
          </ul>
        </div>
      )}

      {command.examples.length > 0 && (
        <div>
          <h4 className="text-discord-textMuted text-xs font-semibold uppercase tracking-wide mb-2">Examples</h4>
          <ul className="space-y-3">
            {command.examples.map((ex, i) => (
              <li key={i} className="border border-discord-border rounded-lg p-3 bg-discord-darker/50">
                <p className="text-discord-textMuted text-sm mb-2">{ex.description}</p>
                <pre className="p-2 rounded bg-discord-darkest border border-discord-border text-discord-text text-sm font-mono overflow-x-auto mb-2">
                  {ex.command}
                </pre>
                <button
                  type="button"
                  onClick={() => onInsertCommand(ex.command)}
                  className="px-3 py-1.5 rounded-lg text-sm font-medium bg-discord-accent text-white hover:bg-discord-accentHover transition-colors"
                >
                  Insert Into Terminal
                </button>
              </li>
            ))}
          </ul>
        </div>
      )}

      {command.examples.length === 0 && (
        <p className="text-discord-textMuted text-sm">No examples for this command.</p>
      )}
    </div>
  );
}

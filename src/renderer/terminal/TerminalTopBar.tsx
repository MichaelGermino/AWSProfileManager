/**
 * Top bar for the Terminal screen (title, profile dropdown, shell dropdown, and optional actions).
 */

import type { Profile } from '../../shared/types';
import { Tooltip } from '../components/Tooltip';

export type TerminalShell = 'powershell' | 'bash';

interface TerminalTopBarProps {
  title?: string;
  /** Profiles for the AWS CLI --profile dropdown; when selected, insert commands append --profile <credentialProfileName>. */
  profiles?: Profile[];
  selectedProfileId?: string | null;
  onProfileChange?: (profileId: string | null) => void;
  /** Appends " --profile <name>" to whatever is currently typed in the terminal.
   *  When set, shows a small icon button beside the profile dropdown. */
  onInsertProfileFlag?: () => void;
  /** Current terminal shell. When set, shows a shell dropdown. */
  terminalShell?: TerminalShell;
  /** Bash executable path from settings. Bash option is disabled when empty. */
  bashPath?: string;
  onShellChange?: (shell: TerminalShell) => void;
}

export function TerminalTopBar({
  title = 'AWS Terminal',
  profiles = [],
  selectedProfileId = null,
  onProfileChange,
  onInsertProfileFlag,
  terminalShell = 'powershell',
  bashPath = '',
  onShellChange,
}: TerminalTopBarProps) {
  const platform = window.electron?.platform;
  // Windows needs a Git Bash path; every other platform ships /bin/bash.
  const bashAvailable = platform !== 'win32' || !!bashPath?.trim();
  // The 'powershell' value means "the system shell" â€” PowerShell on Windows, the login shell
  // (zsh) on macOS. See getShellAndArgs in ptyService.ts.
  const systemShellLabel = platform === 'win32' ? 'PowerShell' : 'Default shell';

  const selectedProfile = profiles.find((p) => p.id === selectedProfileId) ?? null;
  // Matches what handleInsertCommand appends, so the button and Insert agree.
  const profileFlagName =
    selectedProfile?.credentialProfileName?.trim() || selectedProfile?.name?.trim() || '';

  return (
    <div className="flex-shrink-0 flex items-center h-12 px-4 bg-ui-page border-b border-ui-border gap-4">
      <svg
        className="h-[18px] w-[18px] text-ui-textMuted flex-shrink-0"
        fill="none"
        stroke="currentColor"
        viewBox="0 0 24 24"
        aria-hidden
      >
        <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M8 9l3 3-3 3m5 0h3M5 20h14a2 2 0 002-2V6a2 2 0 00-2-2H5a2 2 0 00-2 2v12a2 2 0 002 2z" />
      </svg>
      <span className="text-sm font-medium text-ui-text">{title}</span>

      {onShellChange && (
        <Tooltip label="Terminal shell" placement="below">
          <div className="flex items-center gap-2">
            <label htmlFor="terminal-shell-select" className="text-sm text-ui-textMuted whitespace-nowrap">
              Shell
            </label>
            <select
              id="terminal-shell-select"
              value={terminalShell}
              onChange={(e) => onShellChange(e.target.value as TerminalShell)}
              className="input h-7 w-auto px-2"
              aria-label="Terminal shell"
            >
              <option value="powershell">{systemShellLabel}</option>
              {bashAvailable && <option value="bash">Bash</option>}
            </select>
          </div>
        </Tooltip>
      )}

      {onProfileChange && (
        <div className="flex items-center gap-1.5">
          <Tooltip label="AWS CLI profile for inserted commands" placement="below">
            <div className="flex items-center gap-2">
              <label htmlFor="terminal-profile-select" className="text-sm text-ui-textMuted whitespace-nowrap">
                Profile
              </label>
              <select
                id="terminal-profile-select"
                value={selectedProfileId ?? ''}
                onChange={(e) => onProfileChange(e.target.value ? e.target.value : null)}
                className="input h-7 w-auto px-2"
                aria-label="AWS CLI profile for inserted commands"
              >
                <option value="">No profile</option>
                {profiles.map((p) => (
                  <option key={p.id} value={p.id}>
                    {p.name}
                  </option>
                ))}
              </select>
            </div>
          </Tooltip>

          {onInsertProfileFlag && (
            <Tooltip
              label={
                profileFlagName
                  ? `Append --profile ${profileFlagName} to the terminal`
                  : 'Select a profile to append --profile'
              }
              placement="below"
            >
              <button
                type="button"
                onClick={onInsertProfileFlag}
                disabled={!profileFlagName}
                aria-label={
                  profileFlagName ? `Append --profile ${profileFlagName} to the terminal` : 'Append --profile'
                }
                className="btn-icon h-7 w-7 border border-ui-border"
              >
                <svg className="h-3.5 w-3.5" fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden>
                  <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M12 4v16m8-8H4" />
                </svg>
              </button>
            </Tooltip>
          )}
        </div>
      )}

      <Tooltip label="Open AWS CLI documentation" placement="below">
        <a
          href="https://docs.aws.amazon.com/cli/latest/"
          target="_blank"
          rel="noopener noreferrer"
          onClick={(e) => {
            e.preventDefault();
            window.electron?.openExternal?.('https://docs.aws.amazon.com/cli/latest/');
          }}
          className="ml-auto text-sm text-ui-textMuted hover:text-ui-text transition-colors"
        >
          AWS CLI docs
        </a>
      </Tooltip>
    </div>
  );
}

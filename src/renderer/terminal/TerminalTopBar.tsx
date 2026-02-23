/**
 * Top bar for the Terminal screen (title, profile dropdown, and optional actions).
 */

import type { Profile } from '../../shared/types';
import { Tooltip } from '../components/Tooltip';

interface TerminalTopBarProps {
  title?: string;
  /** Profiles for the AWS CLI --profile dropdown; when selected, insert commands append --profile <credentialProfileName>. */
  profiles?: Profile[];
  selectedProfileId?: string | null;
  onProfileChange?: (profileId: string | null) => void;
}

export function TerminalTopBar({
  title = 'AWS Terminal',
  profiles = [],
  selectedProfileId = null,
  onProfileChange,
}: TerminalTopBarProps) {
  return (
    <div className="flex-shrink-0 flex items-center h-10 px-4 bg-discord-sidebar border-b border-discord-border gap-4">
      <svg
        className="h-5 w-5 text-discord-accent flex-shrink-0"
        fill="none"
        stroke="currentColor"
        viewBox="0 0 24 24"
        aria-hidden
      >
        <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M8 9l3 3-3 3m5 0h3M5 20h14a2 2 0 002-2V6a2 2 0 00-2-2H5a2 2 0 00-2 2v12a2 2 0 002 2z" />
      </svg>
      <span className="text-sm font-semibold text-discord-text">{title}</span>

      {onProfileChange && (
        <Tooltip label="AWS CLI profile for inserted commands" placement="below">
          <div className="flex items-center gap-2">
            <label htmlFor="terminal-profile-select" className="text-xs text-discord-textMuted whitespace-nowrap">
              Profile
            </label>
            <select
              id="terminal-profile-select"
              value={selectedProfileId ?? ''}
              onChange={(e) => onProfileChange(e.target.value ? e.target.value : null)}
              className="px-2.5 py-1 rounded-md bg-discord-darker border border-discord-border text-discord-text text-sm focus:border-discord-accent focus:ring-1 focus:ring-discord-accent"
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
          className="ml-auto text-xs text-discord-textMuted hover:text-discord-accent transition-colors"
        >
          AWS CLI Docs →
        </a>
      </Tooltip>
    </div>
  );
}

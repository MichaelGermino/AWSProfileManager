/**
 * AWS Terminal screen: browse AWS CLI commands, view docs/examples,
 * run commands in an embedded terminal, and use AI to generate examples.
 */

import { useState, useCallback, useEffect } from 'react';
import { useNavigate } from 'react-router-dom';
import { TerminalTopBar } from '../terminal/TerminalTopBar';
import { CommandExplorer } from '../terminal/CommandExplorer';
import { CommandDetailsPanel } from '../terminal/CommandDetailsPanel';
import { EmbeddedTerminal } from '../terminal/EmbeddedTerminal';
import { AIAssistantPanel } from '../terminal/AIAssistantPanel';
import { insertCommandToTerminal } from '../terminal/insertCommandToTerminal';
import { getTerminalApi } from '../terminal/terminalRefStore';
import type { AwsCliCommand } from '../terminal/awsCliMockData';
import type { Profile } from '../shared/types';

declare global {
  interface Window {
    electron: {
      getProfiles?: () => Promise<Profile[]>;
      terminalStart?: () => Promise<void>;
      terminalWrite?: (data: string) => Promise<void>;
      terminalResize?: (cols: number, rows: number) => Promise<void>;
      onTerminalData?: (cb: (data: string) => void) => (() => void) | void;
      onTerminalError?: (cb: (message: string) => void) => (() => void) | void;
      generateAwsCli?: (payload: { prompt: string }) => Promise<{ command: string; explanation: string }>;
      getAiConfigStatus?: () => Promise<{ configured: boolean }>;
      openExternal?: (url: string) => Promise<void>;
    };
  }
}

interface TerminalScreenProps {
  /** When true, the terminal tab is visible (used to refit xterm after being hidden). */
  isVisible?: boolean;
}

export default function TerminalScreen({ isVisible = true }: TerminalScreenProps) {
  const navigate = useNavigate();
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [selectedProfileId, setSelectedProfileId] = useState<string | null>(null);
  const [selectedCommand, setSelectedCommand] = useState<AwsCliCommand | null>(null);
  const [externalAIPrompt, setExternalAIPrompt] = useState<string | null>(null);
  const [aiConfigured, setAiConfigured] = useState(false);

  useEffect(() => {
    window.electron?.getProfiles?.().then((list) => setProfiles(list ?? []));
  }, []);

  useEffect(() => {
    window.electron?.getAiConfigStatus?.().then((r) => setAiConfigured(r?.configured ?? false));
  }, [isVisible]);

  const handleInsertCommand = useCallback(
    (command: string) => {
      const api = getTerminalApi();
      let cmd = command.trim();
      if (selectedProfileId && cmd && !cmd.includes('--profile')) {
        const profile = profiles.find((p) => p.id === selectedProfileId);
        const sectionName = profile?.credentialProfileName?.trim() || profile?.name?.trim();
        if (sectionName) cmd = cmd + ' --profile ' + sectionName;
      }
      insertCommandToTerminal(api, cmd);
    },
    [selectedProfileId, profiles]
  );

  const handleAskAI = useCallback((prompt: string) => {
    setExternalAIPrompt(prompt);
  }, []);

  return (
    <div
      className="flex flex-col bg-discord-darkest h-[calc(100vh-6rem)] max-h-[calc(100vh-6rem)] overflow-hidden"
      style={{ minHeight: 0 }}
    >
      <TerminalTopBar
        title="AWS Terminal"
        profiles={profiles}
        selectedProfileId={selectedProfileId}
        onProfileChange={setSelectedProfileId}
      />

      {/* Grid: fixed-height row so explorer/terminal/AI don't push Command Details off screen */}
      <div className="flex-1 min-h-0 grid grid-cols-1 lg:grid-cols-12 gap-x-0.5 gap-y-0 p-2" style={{ gridTemplateRows: '1fr' }}>
        {/* Left: Command Explorer - scrolls inside */}
        <aside className="lg:col-span-3 flex flex-col min-h-0 overflow-hidden bg-discord-sidebar rounded-lg border border-discord-border">
          <div className="flex-shrink-0 p-2 border-b border-discord-border">
            <h2 className="text-sm font-semibold text-discord-text">Command Explorer</h2>
          </div>
          <div className="flex-1 min-h-0 overflow-y-auto overflow-x-hidden">
            <CommandExplorer onSelectCommand={setSelectedCommand} />
          </div>
        </aside>

        {/* Center: Embedded Terminal */}
        <section className="lg:col-span-6 flex flex-col min-h-0 rounded-lg border border-discord-border overflow-hidden bg-discord-darkest">
          <div className="flex-1 min-h-0 flex flex-col">
            <EmbeddedTerminal className="flex-1 min-h-0" isVisible={isVisible} />
          </div>
        </section>

        {/* Right: AI Assistant */}
        <aside className="lg:col-span-3 flex flex-col min-h-0 rounded-lg border border-discord-border overflow-hidden">
          <AIAssistantPanel
            onInsertCommand={handleInsertCommand}
            externalPrompt={externalAIPrompt}
            onExternalPromptSent={() => setExternalAIPrompt(null)}
            aiConfigured={aiConfigured}
            onOpenSettings={() => navigate('/settings')}
          />
        </aside>
      </div>

      {/* Bottom: Command Details - more vertical space for syntax, options, examples */}
      <div className="flex-shrink-0 h-80 border-t border-discord-border bg-discord-sidebar overflow-hidden">
        <div className="h-full flex flex-col">
          <div className="flex-shrink-0 px-4 py-2 border-b border-discord-border">
            <h2 className="text-sm font-semibold text-discord-text">Command Details</h2>
          </div>
          <div className="flex-1 min-h-0 overflow-auto">
            <CommandDetailsPanel
              command={selectedCommand}
              onInsertCommand={handleInsertCommand}
              onAskAI={aiConfigured ? handleAskAI : undefined}
            />
          </div>
        </div>
      </div>
    </div>
  );
}

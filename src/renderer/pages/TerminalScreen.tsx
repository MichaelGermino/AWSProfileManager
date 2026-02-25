/**
 * AWS Terminal screen: browse AWS CLI commands, view docs/examples,
 * run commands in an embedded terminal, and use AI to generate examples.
 * Panels are resizable; Command Explorer and AI Assistant can be collapsed.
 */

import { useState, useCallback, useEffect, useRef } from 'react';
import { useNavigate } from 'react-router-dom';
import { TerminalTopBar } from '../terminal/TerminalTopBar';
import { CommandExplorer } from '../terminal/CommandExplorer';
import { CommandDetailsPanel } from '../terminal/CommandDetailsPanel';
import { EmbeddedTerminal } from '../terminal/EmbeddedTerminal';
import { AIAssistantPanel } from '../terminal/AIAssistantPanel';
import { insertCommandToTerminal } from '../terminal/insertCommandToTerminal';
import { getTerminalApi } from '../terminal/terminalRefStore';
import { Tooltip } from '../components/Tooltip';
import type { AwsCliCommand } from '../terminal/awsCliMockData';
import type { Profile } from '../shared/types';

const LAYOUT_STORAGE_KEY = 'terminal-layout';
const MIN_PANEL_PERCENT = 15;
const MAX_PANEL_PERCENT = 45;
const MIN_CENTER_PERCENT = 30;
const COLLAPSED_WIDTH_PX = 40;
const DEFAULT_DETAILS_HEIGHT_PX = 320;
const MIN_DETAILS_HEIGHT_PX = 120;
const MAX_DETAILS_HEIGHT_PX = 600;
const COLLAPSED_DETAILS_HEIGHT_PX = 36;

function loadLayout(): {
  leftSize: number;
  rightSize: number;
  leftCollapsed: boolean;
  rightCollapsed: boolean;
  detailsHeight: number;
  detailsCollapsed: boolean;
} {
  try {
    const raw = localStorage.getItem(LAYOUT_STORAGE_KEY);
    if (raw) {
      const data = JSON.parse(raw);
      if (
        typeof data.leftSize === 'number' &&
        typeof data.rightSize === 'number' &&
        typeof data.leftCollapsed === 'boolean' &&
        typeof data.rightCollapsed === 'boolean'
      ) {
        const detailsHeight =
          typeof data.detailsHeight === 'number'
            ? Math.max(MIN_DETAILS_HEIGHT_PX, Math.min(MAX_DETAILS_HEIGHT_PX, data.detailsHeight))
            : DEFAULT_DETAILS_HEIGHT_PX;
        return {
          leftSize: Math.max(0.15, Math.min(0.45, data.leftSize)),
          rightSize: Math.max(0.15, Math.min(0.45, data.rightSize)),
          leftCollapsed: data.leftCollapsed,
          rightCollapsed: data.rightCollapsed,
          detailsHeight,
          detailsCollapsed: typeof data.detailsCollapsed === 'boolean' ? data.detailsCollapsed : false,
        };
      }
    }
  } catch {
    // ignore
  }
  return {
    leftSize: 0.25,
    rightSize: 0.25,
    leftCollapsed: false,
    rightCollapsed: false,
    detailsHeight: DEFAULT_DETAILS_HEIGHT_PX,
    detailsCollapsed: false,
  };
}

function saveLayout(layout: ReturnType<typeof loadLayout>) {
  try {
    localStorage.setItem(LAYOUT_STORAGE_KEY, JSON.stringify(layout));
  } catch {
    // ignore
  }
}

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
  isVisible?: boolean;
}

export default function TerminalScreen({ isVisible = true }: TerminalScreenProps) {
  const navigate = useNavigate();
  const [profiles, setProfiles] = useState<Profile[]>([]);
  const [selectedProfileId, setSelectedProfileId] = useState<string | null>(null);
  const [selectedCommand, setSelectedCommand] = useState<AwsCliCommand | null>(null);
  const [externalAIPrompt, setExternalAIPrompt] = useState<string | null>(null);
  const [aiConfigured, setAiConfigured] = useState(false);

  const [layout, setLayout] = useState(loadLayout);
  const containerRef = useRef<HTMLDivElement>(null);
  const dragRef = useRef<{
    side: 'left' | 'right';
    startX: number;
    startLeft: number;
    startRight: number;
    currentLeft: number;
    currentRight: number;
  } | null>(null);
  const detailsPanelRef = useRef<HTMLDivElement>(null);
  const detailsDragRef = useRef<{ startY: number; startHeight: number; currentHeight: number } | null>(null);

  useEffect(() => {
    saveLayout(layout);
  }, [layout]);

  useEffect(() => {
    if (!isVisible) return;
    window.electron?.getProfiles?.().then((list) => setProfiles(list ?? []));
  }, [isVisible]);

  useEffect(() => {
    window.electron?.getAiConfigStatus?.().then((r) => setAiConfigured(r?.configured ?? false));
  }, [isVisible]);

  const handleResizeStart = useCallback((side: 'left' | 'right', clientX: number) => {
    const currentLeft = layout.leftSize;
    const currentRight = layout.rightSize;
    dragRef.current = {
      side,
      startX: clientX,
      startLeft: currentLeft,
      startRight: currentRight,
      currentLeft,
      currentRight,
    };
    document.body.style.userSelect = 'none';
    document.body.style.cursor = 'col-resize';

    const applyWidths = (left: number, right: number) => {
      const el = containerRef.current;
      if (el) {
        el.style.setProperty('--terminal-left-pct', `${left * 100}%`);
        el.style.setProperty('--terminal-right-pct', `${right * 100}%`);
      }
    };

    const onMove = (e: MouseEvent) => {
      const ref = dragRef.current;
      const el = containerRef.current;
      if (!ref || !el) return;
      const w = el.getBoundingClientRect().width;
      if (w <= 0) return;
      const dx = (e.clientX - ref.startX) / w;
      if (ref.side === 'left') {
        const minLeft = MIN_PANEL_PERCENT / 100;
        const maxLeft = (100 - MIN_CENTER_PERCENT - ref.startRight * 100) / 100;
        ref.currentLeft = Math.max(minLeft, Math.min(maxLeft, ref.startLeft + dx));
        ref.currentRight = ref.startRight;
        applyWidths(ref.currentLeft, ref.currentRight);
      } else {
        const minRight = MIN_PANEL_PERCENT / 100;
        const maxRight = (100 - MIN_CENTER_PERCENT - ref.startLeft * 100) / 100;
        ref.currentRight = Math.max(minRight, Math.min(maxRight, ref.startRight - dx));
        ref.currentLeft = ref.startLeft;
        applyWidths(ref.currentLeft, ref.currentRight);
      }
    };

    const onUp = () => {
      window.removeEventListener('mousemove', onMove, { capture: true });
      window.removeEventListener('mouseup', onUp, { capture: true });
      const ref = dragRef.current;
      if (ref) {
        setLayout((prev) => ({
          ...prev,
          leftSize: ref.currentLeft,
          rightSize: ref.currentRight,
        }));
      }
      dragRef.current = null;
      document.body.style.userSelect = '';
      document.body.style.cursor = '';
    };

    window.addEventListener('mousemove', onMove, { capture: true });
    window.addEventListener('mouseup', onUp, { capture: true });
  }, [layout.leftSize, layout.rightSize]);

  const handleDetailsResizeStart = useCallback((clientY: number) => {
    detailsDragRef.current = {
      startY: clientY,
      startHeight: layout.detailsHeight,
      currentHeight: layout.detailsHeight,
    };
    document.body.style.userSelect = 'none';
    document.body.style.cursor = 'row-resize';

    const applyHeight = (h: number) => {
      const el = detailsPanelRef.current;
      if (el) el.style.setProperty('height', `${h}px`);
    };

    const onMove = (e: MouseEvent) => {
      const ref = detailsDragRef.current;
      if (!ref) return;
      const dy = ref.startY - e.clientY;
      ref.currentHeight = Math.max(
        MIN_DETAILS_HEIGHT_PX,
        Math.min(MAX_DETAILS_HEIGHT_PX, ref.startHeight + dy)
      );
      applyHeight(ref.currentHeight);
    };

    const onUp = () => {
      window.removeEventListener('mousemove', onMove, { capture: true });
      window.removeEventListener('mouseup', onUp, { capture: true });
      const ref = detailsDragRef.current;
      if (ref) setLayout((prev) => ({ ...prev, detailsHeight: ref.currentHeight }));
      detailsDragRef.current = null;
      document.body.style.userSelect = '';
      document.body.style.cursor = '';
    };

    window.addEventListener('mousemove', onMove, { capture: true });
    window.addEventListener('mouseup', onUp, { capture: true });
  }, [layout.detailsHeight]);

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

  const collapseButton = (collapsed: boolean, onToggle: () => void, label: string) => {
    const isLeft = label === 'Command Explorer';
    const showChevronRight = (isLeft && collapsed) || (!isLeft && !collapsed);
    return (
      <Tooltip label={collapsed ? `Expand ${label}` : `Collapse ${label}`} placement={isLeft ? 'right' : 'left'}>
        <button
          type="button"
          onClick={onToggle}
          className="flex-shrink-0 p-1.5 rounded-md text-discord-textMuted hover:text-discord-text hover:bg-discord-panel transition-colors"
          aria-label={collapsed ? `Expand ${label}` : `Collapse ${label}`}
        >
          <svg className="h-4 w-4" fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden>
            {showChevronRight ? (
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M13 5l7 7-7 7M5 5l7 7-7 7" />
            ) : (
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M11 19l-7-7 7-7m8 14l-7-7 7-7" />
            )}
          </svg>
        </button>
      </Tooltip>
    );
  };

  const detailsCollapseButton = (collapsed: boolean, onToggle: () => void) => (
    <Tooltip label={collapsed ? 'Expand Command Details' : 'Collapse Command Details'} placement="top">
      <button
        type="button"
        onClick={onToggle}
        className="flex-shrink-0 p-1.5 rounded-md text-discord-textMuted hover:text-discord-text hover:bg-discord-panel transition-colors"
        aria-label={collapsed ? 'Expand Command Details' : 'Collapse Command Details'}
      >
        <svg className="h-4 w-4" fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden>
          {collapsed ? (
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M5 15l7-7 7 7" />
          ) : (
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M19 9l-7 7-7-7" />
          )}
        </svg>
      </button>
    </Tooltip>
  );

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

      <div
        ref={containerRef}
        className="flex-1 min-h-0 flex flex-nowrap items-stretch gap-0 p-2"
        style={{
          minHeight: 0,
          ['--terminal-left-pct' as string]: `${layout.leftSize * 100}%`,
          ['--terminal-right-pct' as string]: `${layout.rightSize * 100}%`,
        }}
      >
        {/* Left: Command Explorer (resizable, collapsible) */}
        <aside
          className="flex flex-col min-h-0 overflow-hidden bg-discord-sidebar rounded-lg border border-discord-border flex-shrink-0"
          style={{
            width: layout.leftCollapsed ? COLLAPSED_WIDTH_PX : 'var(--terminal-left-pct)',
            minWidth: layout.leftCollapsed ? COLLAPSED_WIDTH_PX : undefined,
            transition: layout.leftCollapsed ? 'width 0.2s ease-out' : 'none',
          }}
        >
          {layout.leftCollapsed ? (
            <div className="flex flex-col items-center h-full py-2 border-r border-discord-border">
              {collapseButton(layout.leftCollapsed, () => setLayout((p) => ({ ...p, leftCollapsed: false })), 'Command Explorer')}
              <span className="mt-2 text-[10px] text-discord-textMuted uppercase tracking-wider whitespace-nowrap [writing-mode:vertical-rl] rotate-180">
                Command Explorer
              </span>
            </div>
          ) : (
            <>
              <div className="flex-shrink-0 p-2 border-b border-discord-border flex items-center gap-1">
                {collapseButton(false, () => setLayout((p) => ({ ...p, leftCollapsed: true })), 'Command Explorer')}
                <h2 className="text-sm font-semibold text-discord-text">Command Explorer</h2>
              </div>
              <div className="flex-1 min-h-0 overflow-y-auto overflow-x-hidden">
                <CommandExplorer onSelectCommand={setSelectedCommand} />
              </div>
            </>
          )}
        </aside>

        {/* Resize handle between left and center */}
        {!layout.leftCollapsed && (
          <div
            className="flex-shrink-0 w-1 cursor-col-resize hover:bg-discord-accent/30 active:bg-discord-accent/50 group flex items-center justify-center select-none"
            onMouseDown={(e) => {
              if (e.button === 0) {
                e.preventDefault();
                handleResizeStart('left', e.clientX);
              }
            }}
            role="separator"
            aria-label="Resize Command Explorer"
          >
            <div className="w-0.5 h-8 rounded-full bg-discord-border group-hover:bg-discord-accent/80 opacity-0 group-hover:opacity-100 transition-opacity pointer-events-none" />
          </div>
        )}

        {/* Center: Embedded Terminal */}
        <section
          className="flex flex-col min-h-0 rounded-lg border border-discord-border overflow-hidden bg-discord-darkest flex-1"
          style={{ minWidth: 300 }}
        >
          <div className="flex-1 min-h-0 flex flex-col">
            <EmbeddedTerminal className="flex-1 min-h-0" isVisible={isVisible} />
          </div>
        </section>

        {/* Resize handle between center and right */}
        {!layout.rightCollapsed && (
          <div
            className="flex-shrink-0 w-1 cursor-col-resize hover:bg-discord-accent/30 active:bg-discord-accent/50 group flex items-center justify-center select-none"
            onMouseDown={(e) => {
              if (e.button === 0) {
                e.preventDefault();
                handleResizeStart('right', e.clientX);
              }
            }}
            role="separator"
            aria-label="Resize AI Assistant"
          >
            <div className="w-0.5 h-8 rounded-full bg-discord-border group-hover:bg-discord-accent/80 opacity-0 group-hover:opacity-100 transition-opacity pointer-events-none" />
          </div>
        )}

        {/* Right: AI Assistant (resizable, collapsible) */}
        <aside
          className="flex flex-col min-h-0 overflow-hidden rounded-lg border border-discord-border flex-shrink-0"
          style={{
            width: layout.rightCollapsed ? COLLAPSED_WIDTH_PX : 'var(--terminal-right-pct)',
            minWidth: layout.rightCollapsed ? COLLAPSED_WIDTH_PX : undefined,
            transition: layout.rightCollapsed ? 'width 0.2s ease-out' : 'none',
          }}
        >
          {layout.rightCollapsed ? (
            <div className="flex flex-col items-center h-full py-2 bg-discord-darker/50 border border-discord-border rounded-lg">
              {collapseButton(layout.rightCollapsed, () => setLayout((p) => ({ ...p, rightCollapsed: false })), 'AI Assistant')}
              <span className="mt-2 text-[10px] text-discord-textMuted uppercase tracking-wider whitespace-nowrap [writing-mode:vertical-rl] rotate-180">
                AI Assistant
              </span>
            </div>
          ) : (
            <AIAssistantPanel
              onInsertCommand={handleInsertCommand}
              externalPrompt={externalAIPrompt}
              onExternalPromptSent={() => setExternalAIPrompt(null)}
              aiConfigured={aiConfigured}
              onOpenSettings={() => navigate('/settings')}
              headerLeftContent={collapseButton(
                false,
                () => setLayout((p) => ({ ...p, rightCollapsed: true })),
                'AI Assistant'
              )}
            />
          )}
        </aside>
      </div>

      {/* Resize handle above Command Details */}
      {!layout.detailsCollapsed && (
        <div
          className="flex-shrink-0 h-1.5 cursor-row-resize hover:bg-discord-accent/30 active:bg-discord-accent/50 flex items-center justify-center group select-none"
          onMouseDown={(e) => {
            if (e.button === 0) {
              e.preventDefault();
              handleDetailsResizeStart(e.clientY);
            }
          }}
          role="separator"
          aria-label="Resize Command Details"
        >
          <div className="h-0.5 w-12 rounded-full bg-discord-border group-hover:bg-discord-accent/80 opacity-0 group-hover:opacity-100 transition-opacity pointer-events-none" />
        </div>
      )}

      {/* Bottom: Command Details (resizable, collapsible) */}
      <div
        ref={detailsPanelRef}
        className="flex-shrink-0 border-t border-discord-border bg-discord-sidebar overflow-hidden flex flex-col"
        style={{
          height: layout.detailsCollapsed ? COLLAPSED_DETAILS_HEIGHT_PX : layout.detailsHeight,
          transition: layout.detailsCollapsed ? 'height 0.2s ease-out' : 'none',
        }}
      >
        {layout.detailsCollapsed ? (
          <div className="flex items-center justify-center gap-2 h-full border-t border-discord-border">
            {detailsCollapseButton(true, () => setLayout((p) => ({ ...p, detailsCollapsed: false })))}
            <span className="text-xs text-discord-textMuted font-semibold uppercase tracking-wider">
              Command Details
            </span>
          </div>
        ) : (
          <>
            <div className="flex-shrink-0 px-4 py-2 border-b border-discord-border flex items-center gap-2">
              {detailsCollapseButton(false, () => setLayout((p) => ({ ...p, detailsCollapsed: true })))}
              <h2 className="text-sm font-semibold text-discord-text">Command Details</h2>
            </div>
            <div className="flex-1 min-h-0 overflow-auto">
              <CommandDetailsPanel
                command={selectedCommand}
                onInsertCommand={handleInsertCommand}
                onAskAI={aiConfigured ? handleAskAI : undefined}
              />
            </div>
          </>
        )}
      </div>
    </div>
  );
}

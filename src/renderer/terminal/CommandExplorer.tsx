/**
 * Searchable tree view of AWS CLI commands.
 * Empty search: full tree. With search: show all matching commands (any service).
 */

import { useState, useMemo, useCallback } from 'react';
import type { AwsCliCommand } from './awsCliMockData';
import { AWS_CLI_MOCK_TREE } from './awsCliMockData';
import { Tooltip } from '../components/Tooltip';

interface CommandExplorerProps {
  searchPlaceholder?: string;
  onSelectCommand: (cmd: AwsCliCommand | null) => void;
}

/** Collect every selectable command (leaf) that matches the query, plus parent path for display. */
function getMatchingCommands(nodes: AwsCliCommand[], query: string): Array<{ cmd: AwsCliCommand; parentName: string }> {
  const q = query.trim().toLowerCase();
  const out: Array<{ cmd: AwsCliCommand; parentName: string }> = [];

  function visit(node: AwsCliCommand, parentName: string) {
    const nameMatch = !q || node.name.toLowerCase().includes(q);
    const descMatch = !q || node.description.toLowerCase().includes(q);
    const matches = nameMatch || descMatch;

    if (node.children && node.children.length > 0) {
      node.children.forEach((c) => visit(c, node.name));
      if (!q) return; // when no search, we render tree; this fn used only for search results
    } else {
      if (matches) out.push({ cmd: node, parentName });
    }
  }

  nodes.forEach((node) => visit(node, ''));
  return out;
}

/** Full tree: render service groups with nested commands. */
function TreeView({
  nodes,
  selectedId,
  onSelect,
}: {
  nodes: AwsCliCommand[];
  selectedId: string | null;
  onSelect: (cmd: AwsCliCommand) => void;
}) {
  return (
    <ul className="space-y-1 list-none pl-0">
      {nodes.map((node) => {
        const hasChildren = node.children && node.children.length > 0;
        const isSelected = selectedId === node.id;

        if (hasChildren) {
          return (
            <li key={node.id} className="pl-0">
              <div className="px-3 py-2 rounded-md text-discord-textMuted text-xs font-semibold uppercase tracking-wider bg-discord-darkest/60 border-l-2 border-discord-accent/50">
                {node.name}
              </div>
              <ul className="list-none pl-0 mt-1 ml-2 border-l border-discord-border pl-3 space-y-0.5">
                {node.children!.map((child) => {
                  const childSelected = selectedId === child.id;
                  return (
                    <li key={child.id}>
                      <Tooltip label={child.description} placement="right" wrap wrapWidth="md">
                        <button
                          type="button"
                          onClick={() => onSelect(child)}
                          className={`w-full text-left px-3 py-2 rounded-md text-sm transition-colors flex items-center gap-2 ${
                            childSelected
                              ? 'bg-discord-accent/25 text-discord-text border border-discord-accent/50'
                              : 'text-discord-textMuted hover:bg-discord-panel hover:text-discord-text'
                          }`}
                        >
                          <span className="font-mono text-discord-accent">{child.name}</span>
                        </button>
                      </Tooltip>
                    </li>
                  );
                })}
              </ul>
            </li>
          );
        }

        return (
          <li key={node.id}>
            <Tooltip label={node.description} placement="right" wrap wrapWidth="md">
              <button
                type="button"
                onClick={() => onSelect(node)}
                className={`w-full text-left px-3 py-2 rounded-md text-sm transition-colors ${
                  isSelected
                    ? 'bg-discord-accent/25 text-discord-text border border-discord-accent/50'
                    : 'text-discord-textMuted hover:bg-discord-panel hover:text-discord-text'
                }`}
              >
                <span className="font-mono text-discord-accent">{node.name}</span>
              </button>
            </Tooltip>
          </li>
        );
      })}
    </ul>
  );
}

/** Search results: flat list of "service > command" with description. */
function SearchResults({
  matches,
  selectedId,
  onSelect,
}: {
  matches: Array<{ cmd: AwsCliCommand; parentName: string }>;
  selectedId: string | null;
  onSelect: (cmd: AwsCliCommand) => void;
}) {
  if (matches.length === 0) {
    return (
      <p className="px-4 py-6 text-discord-textMuted text-sm">
        No commands match. Try &quot;lambda&quot;, &quot;s3&quot;, &quot;list&quot;, &quot;describe&quot;, etc.
      </p>
    );
  }

  return (
    <ul className="list-none pl-0 space-y-0.5">
      {matches.map(({ cmd, parentName }) => {
        const isSelected = selectedId === cmd.id;
        return (
          <li key={cmd.id}>
            <Tooltip label={cmd.description} placement="right" wrap wrapWidth="md">
              <button
                type="button"
                onClick={() => onSelect(cmd)}
                className={`w-full text-left px-3 py-2.5 rounded-md text-sm transition-colors border ${
                  isSelected
                    ? 'bg-discord-accent/25 text-discord-text border-discord-accent/50'
                    : 'border-transparent text-discord-textMuted hover:bg-discord-panel hover:text-discord-text hover:border-discord-border'
                }`}
              >
                <span className="font-mono text-discord-accent">
                  {parentName ? `${parentName} ${cmd.name}` : cmd.name}
                </span>
                <p className="text-xs text-discord-textMuted mt-1 truncate">{cmd.description}</p>
              </button>
            </Tooltip>
          </li>
        );
      })}
    </ul>
  );
}

export function CommandExplorer({ searchPlaceholder = 'Search AWS CLI…', onSelectCommand }: CommandExplorerProps) {
  const [query, setQuery] = useState('');
  const [selectedId, setSelectedId] = useState<string | null>(null);

  const handleSelect = useCallback(
    (cmd: AwsCliCommand) => {
      setSelectedId(cmd.id);
      onSelectCommand(cmd);
    },
    [onSelectCommand]
  );

  const searchMatches = useMemo(() => getMatchingCommands(AWS_CLI_MOCK_TREE, query), [query]);
  const hasSearch = query.trim().length > 0;

  return (
    <div className="flex flex-col min-h-0 h-full">
      <div className="flex-shrink-0 p-3 border-b border-discord-border">
        <Tooltip label="Search AWS CLI commands" placement="below">
          <input
            type="search"
            placeholder={searchPlaceholder}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            className="w-full px-3 py-2.5 rounded-lg bg-discord-darker border border-discord-border text-discord-text text-sm placeholder-discord-textMuted focus:border-discord-accent focus:ring-1 focus:ring-discord-accent"
            aria-label="Search AWS CLI commands"
          />
        </Tooltip>
      </div>
      <div className="flex-1 min-h-0 overflow-y-auto overflow-x-hidden p-2">
        {hasSearch ? (
          <SearchResults matches={searchMatches} selectedId={selectedId} onSelect={handleSelect} />
        ) : (
          <TreeView nodes={AWS_CLI_MOCK_TREE} selectedId={selectedId} onSelect={handleSelect} />
        )}
      </div>
    </div>
  );
}

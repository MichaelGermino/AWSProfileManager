/**
 * Searchable tree view of AWS CLI commands.
 * Empty search: full tree (mock data). With search: mock matches + scraped commands for services not in mock.
 */

import { useState, useMemo, useCallback, useEffect, useRef } from 'react';
import type { AwsCliCommand } from './awsCliMockData';
import { AWS_CLI_MOCK_TREE } from './awsCliMockData';
import { Tooltip } from '../components/Tooltip';

declare global {
  interface Window {
    electron?: {
      getAwsCliServiceList?: () => Promise<string[]>;
      getAwsCliCommandsForService?: (slug: string) => Promise<AwsCliCommand[]>;
    };
  }
}

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
              <div className="px-3 pb-1 pt-3 text-sm text-ui-textMuted">
                {node.name}
              </div>
              <ul className="ml-3 list-none space-y-0.5 border-l border-ui-border pl-2">
                {node.children!.map((child) => {
                  const childSelected = selectedId === child.id;
                  return (
                    <li key={child.id}>
                      <Tooltip label={child.description} placement="right" wrap wrapWidth="2xl" usePortal>
                        <button
                          type="button"
                          onClick={() => onSelect(child)}
                          className={`flex h-8 w-full items-center gap-2 rounded-button px-3 text-left text-sm transition-colors ${
 childSelected ? 'bg-ui-raised text-ui-text' : 'text-ui-textMuted hover:bg-ui-hover hover:text-ui-text'
 }`}
                        >
                          <span className="font-mono">{child.name}</span>
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
            <Tooltip label={node.description} placement="right" wrap wrapWidth="2xl" usePortal>
              <button
                type="button"
                onClick={() => onSelect(node)}
                className={`flex h-8 w-full items-center rounded-button px-3 text-left text-sm transition-colors ${
 isSelected ? 'bg-ui-raised text-ui-text' : 'text-ui-textMuted hover:bg-ui-hover hover:text-ui-text'
 }`}
              >
                <span className="font-mono">{node.name}</span>
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
  loading,
}: {
  matches: Array<{ cmd: AwsCliCommand; parentName: string }>;
  selectedId: string | null;
  onSelect: (cmd: AwsCliCommand) => void;
  loading?: boolean;
}) {
  if (matches.length === 0 && !loading) {
    return (
      <p className="px-4 py-6 text-ui-textMuted text-sm">
        No commands match. Try &quot;s3&quot;, &quot;sts&quot;, &quot;lambda&quot;, &quot;list&quot;, or &quot;describe&quot; (built-in). More services appear when AWS CLI docs load.
      </p>
    );
  }

  return (
    <>
      {loading && (
        <div className="flex items-center justify-center py-4" aria-label="Searching">
          <div
            className="h-4 w-4 rounded-full border-2 border-ui-border border-t-ui-textMuted animate-spin"
            role="presentation"
          />
        </div>
      )}
      <ul className="list-none pl-0 space-y-0.5">
        {matches.map(({ cmd, parentName }) => {
          const isSelected = selectedId === cmd.id;
          return (
            <li key={cmd.id}>
              <Tooltip label={cmd.description || cmd.syntax} placement="right" wrap wrapWidth="2xl" usePortal>
                <button
                  type="button"
                  onClick={() => onSelect(cmd)}
                  className={`w-full rounded-button px-3 py-2 text-left text-sm transition-colors ${
 isSelected ? 'bg-ui-raised text-ui-text' : 'text-ui-textMuted hover:bg-ui-hover hover:text-ui-text'
 }`}
                >
                  <span className="font-mono">
                    {parentName ? `${parentName} ${cmd.name}` : cmd.name}
                  </span>
                  {(cmd.description || cmd.syntax) && (
                    <p className="mt-0.5 truncate text-sm text-ui-textFaint">
                      {cmd.description || cmd.syntax}
                    </p>
                  )}
                </button>
              </Tooltip>
            </li>
          );
        })}
      </ul>
    </>
  );
}

/** Top-level service ids from mock tree (so we don't scrape them). */
const MOCK_SERVICE_IDS = new Set(
  AWS_CLI_MOCK_TREE.filter((n) => n.children && n.children.length > 0).map((n) => n.id)
);

export function CommandExplorer({ searchPlaceholder = 'Search commands', onSelectCommand }: CommandExplorerProps) {
  const [query, setQuery] = useState('');
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [serviceList, setServiceList] = useState<string[]>([]);
  const [scrapedMatches, setScrapedMatches] = useState<Array<{ cmd: AwsCliCommand; parentName: string }>>([]);
  const [scrapedLoading, setScrapedLoading] = useState(false);
  const queryRef = useRef(query);
  queryRef.current = query;

  useEffect(() => {
    window.electron
      ?.getAwsCliServiceList?.()
      .then((list) => setServiceList(list ?? []))
      .catch(() => setServiceList([]));
  }, []);

  // Debounce scraped search so typing stays fluid; only fetch after user pauses (and require 2+ chars).
  const SCRAPED_DEBOUNCE_MS = 280;
  const MIN_QUERY_LENGTH_FOR_SCRAPED = 2;

  useEffect(() => {
    const q = query.trim().toLowerCase();
    if (!q) {
      setScrapedMatches([]);
      setScrapedLoading(false);
      return;
    }
    const timer = setTimeout(() => {
      const currentQ = queryRef.current.trim().toLowerCase();
      if (!currentQ || currentQ.length < MIN_QUERY_LENGTH_FOR_SCRAPED) {
        setScrapedMatches([]);
        setScrapedLoading(false);
        return;
      }
      const getCommands = window.electron?.getAwsCliCommandsForService;
      if (!getCommands) {
        setScrapedMatches([]);
        setScrapedLoading(false);
        return;
      }
      const runSearch = (list: string[]) => {
        const matchingServices = list.filter(
          (slug) => slug.toLowerCase().includes(currentQ) && !MOCK_SERVICE_IDS.has(slug)
        );
        if (matchingServices.length === 0) {
          setScrapedMatches([]);
          setScrapedLoading(false);
          return;
        }
        setScrapedLoading(true);
        Promise.all(matchingServices.map((slug) => getCommands(slug)))
          .then((results) => {
            if (queryRef.current.trim().toLowerCase() !== currentQ) return;
            const out: Array<{ cmd: AwsCliCommand; parentName: string }> = [];
            results.forEach((commands, i) => {
              const slug = matchingServices[i];
              const filtered = commands.filter(
                (c) =>
                  c.name.toLowerCase().includes(currentQ) ||
                  (c.description && c.description.toLowerCase().includes(currentQ)) ||
                  (c.syntax && c.syntax.toLowerCase().includes(currentQ))
              );
              filtered.forEach((cmd) => out.push({ cmd, parentName: slug }));
            });
            setScrapedMatches(out);
          })
          .catch(() => {
            if (queryRef.current.trim().toLowerCase() === currentQ) setScrapedMatches([]);
          })
          .finally(() => {
            if (queryRef.current.trim().toLowerCase() === currentQ) setScrapedLoading(false);
          });
      };
      if (serviceList.length > 0) {
        runSearch(serviceList);
      } else {
        setScrapedLoading(true);
        window.electron
          ?.getAwsCliServiceList?.()
          .then((list) => {
            const slugs = list ?? [];
            if (slugs.length > 0) setServiceList(slugs);
            runSearch(slugs);
          })
          .catch(() => {
            if (queryRef.current.trim().toLowerCase() === currentQ) {
              setScrapedMatches([]);
              setScrapedLoading(false);
            }
          });
      }
    }, SCRAPED_DEBOUNCE_MS);
    return () => clearTimeout(timer);
  }, [query, serviceList]);

  const handleSelect = useCallback(
    (cmd: AwsCliCommand) => {
      setSelectedId(cmd.id);
      onSelectCommand(cmd);
    },
    [onSelectCommand]
  );

  const mockMatches = useMemo(() => getMatchingCommands(AWS_CLI_MOCK_TREE, query), [query]);
  const allSearchMatches = useMemo(
    () => [...mockMatches, ...scrapedMatches],
    [mockMatches, scrapedMatches]
  );
  const hasSearch = query.trim().length > 0;

  return (
    <div className="flex flex-col min-h-0 h-full">
      <div className="flex-shrink-0 p-3 border-b border-ui-border">
        <input
          type="search"
          placeholder={searchPlaceholder}
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          className="input"
          aria-label="Search AWS CLI commands"
        />
      </div>
      <div className="flex-1 min-h-0 overflow-y-auto overflow-x-hidden p-2">
        {hasSearch ? (
          <SearchResults
            matches={allSearchMatches}
            selectedId={selectedId}
            onSelect={handleSelect}
            loading={scrapedLoading}
          />
        ) : (
          <TreeView nodes={AWS_CLI_MOCK_TREE} selectedId={selectedId} onSelect={handleSelect} />
        )}
      </div>
    </div>
  );
}

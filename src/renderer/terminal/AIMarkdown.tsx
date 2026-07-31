/**
 * Markdown renderer for AI assistant replies.
 *
 * Replies are GitHub-flavoured Markdown (see AWS_CLI_SYSTEM_PROMPT in aiService.ts), so
 * they arrive with headings, lists, tables and multiple fenced code blocks. Each code
 * block that looks like a shell command gets its own "Insert Into Terminal" button —
 * previously there was a single command per reply, which is why richer answers had
 * nowhere to put their second and third examples.
 */

import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';

/** Minimal hast node shape; react-markdown passes the original node to each component. */
interface HastNode {
  type?: string;
  tagName?: string;
  value?: string;
  properties?: { className?: string[] | string };
  children?: HastNode[];
}

/** Flattens a hast subtree to its raw text — used to recover code-block contents verbatim. */
function hastToText(node: HastNode | undefined): string {
  if (!node) return '';
  if (node.type === 'text') return node.value ?? '';
  if (Array.isArray(node.children)) return node.children.map(hastToText).join('');
  return '';
}

function languageOf(node: HastNode | undefined): string {
  const codeEl = node?.children?.find((c) => c.tagName === 'code');
  const cls = codeEl?.properties?.className;
  const list = Array.isArray(cls) ? cls : typeof cls === 'string' ? [cls] : [];
  const match = list.find((c) => c.startsWith('language-'));
  return match ? match.slice('language-'.length).toLowerCase() : '';
}

/**
 * Whether a code block is something you'd actually run. Output samples (JSON, tables) get
 * no Insert button, so the button means "this is runnable" rather than appearing everywhere.
 *
 * Syntax templates are excluded too: replies usually open with a "Syntax" block like
 * `aws s3 rb s3://<bucket-name> [options]`, and inserting that verbatim just puts
 * placeholder text at the prompt.
 */
function isRunnable(language: string, text: string): boolean {
  const trimmed = text.trim();
  if (!trimmed) return false;

  // <bucket-name> / [options] / <value> — a template, not a command.
  if (/<[^>\s][^>]*>/.test(trimmed) || /\[[a-z][a-z0-9 -]*\]/i.test(trimmed)) return false;

  if (['bash', 'sh', 'shell', 'zsh', 'console', 'powershell', 'ps1'].includes(language)) return true;
  if (language) return false; // an explicit non-shell language (json, yaml, python…)
  const firstLine = trimmed.split('\n')[0] ?? '';
  return /^(aws|export|set)\b/.test(firstLine);
}

export function AIMarkdown({
  content,
  onInsertCommand,
}: {
  content: string;
  onInsertCommand: (command: string) => void;
}) {
  return (
    <div className="text-sm leading-relaxed space-y-2">
      <ReactMarkdown
        remarkPlugins={[remarkGfm]}
        components={{
          // Render the whole fenced block ourselves from the raw text so the Insert button
          // gets the exact command, not React children we'd have to re-flatten.
          pre({ node }) {
            const text = hastToText(node as HastNode).replace(/\n+$/, '');
            const language = languageOf(node as HastNode);
            return (
              <div className="my-2">
                <pre className="p-2 rounded bg-discord-darkest border border-discord-border text-discord-text text-xs font-mono overflow-x-auto">
                  {text}
                </pre>
                {isRunnable(language, text) && (
                  <button
                    type="button"
                    onClick={() => onInsertCommand(text)}
                    className="mt-1.5 px-2 py-1 rounded text-xs font-medium bg-discord-accent text-white hover:bg-discord-accentHover transition-colors"
                  >
                    Insert Into Terminal
                  </button>
                )}
              </div>
            );
          },
          code({ children }) {
            // Only inline code reaches here; fenced blocks are handled by `pre` above.
            return (
              <code className="px-1 py-0.5 rounded bg-discord-darkest border border-discord-border text-discord-text text-xs font-mono">
                {children}
              </code>
            );
          },
          h1: ({ children }) => <h4 className="mt-3 mb-1 text-sm font-bold text-discord-text">{children}</h4>,
          h2: ({ children }) => <h4 className="mt-3 mb-1 text-sm font-bold text-discord-text">{children}</h4>,
          h3: ({ children }) => <h5 className="mt-3 mb-1 text-sm font-semibold text-discord-text">{children}</h5>,
          h4: ({ children }) => <h5 className="mt-3 mb-1 text-sm font-semibold text-discord-text">{children}</h5>,
          p: ({ children }) => <p className="my-1.5">{children}</p>,
          ul: ({ children }) => <ul className="my-1.5 ml-4 list-disc space-y-1">{children}</ul>,
          ol: ({ children }) => <ol className="my-1.5 ml-4 list-decimal space-y-1">{children}</ol>,
          li: ({ children }) => <li className="pl-1">{children}</li>,
          strong: ({ children }) => <strong className="font-semibold text-discord-text">{children}</strong>,
          hr: () => <hr className="my-3 border-discord-border" />,
          blockquote: ({ children }) => (
            <blockquote className="my-2 pl-3 border-l-2 border-discord-border italic">{children}</blockquote>
          ),
          table: ({ children }) => (
            <div className="my-2 overflow-x-auto">
              <table className="text-xs border border-discord-border">{children}</table>
            </div>
          ),
          th: ({ children }) => (
            <th className="border border-discord-border px-2 py-1 text-left font-semibold text-discord-text">
              {children}
            </th>
          ),
          td: ({ children }) => <td className="border border-discord-border px-2 py-1 align-top">{children}</td>,
          a: ({ href, children }) => (
            <a
              href={href}
              onClick={(e) => {
                // Renderer has no nodeIntegration; open in the real browser via main.
                e.preventDefault();
                if (href) window.electron?.openExternal?.(href);
              }}
              className="text-discord-accent hover:underline"
            >
              {children}
            </a>
          ),
        }}
      >
        {content}
      </ReactMarkdown>
    </div>
  );
}

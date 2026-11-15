import { useId } from 'react';
import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';

/**
 * "What's new" after an update, rendering the GitHub release notes for the running version.
 *
 * Lazy-loaded by App: react-markdown and remark-gfm are ~165 kB and this appears at most once per
 * version, so they must not sit in the initial bundle.
 *
 * The Markdown is remote content. react-markdown escapes HTML unless rehype-raw is added — it is
 * not, deliberately — so the notes cannot inject markup. Links are intercepted and handed to the
 * OS browser; letting one navigate the renderer would replace the app with a web page and there is
 * no way back.
 */
export function ChangelogModal({
  version,
  notes,
  url,
  onClose,
  onStartTour,
}: {
  version: string;
  notes: string;
  url: string;
  /** Dismiss. Also what the X and "Skip tour" do — declining the offer is still just closing. */
  onClose: () => void;
  /**
   * Present only when a guided tour is actually due for this version, which turns the single
   * "Got it" into "Skip tour" / "Start tour". A release with no tour keeps "Got it" — offering to
   * start something that does not exist would be worse than not asking.
   */
  onStartTour?: () => void;
}) {
  const titleId = useId();
  return (
    <div className="fixed inset-0 z-[70] flex items-center justify-center modal-backdrop p-4">
      <div role="dialog" aria-modal="true" aria-labelledby={titleId} className="flex max-h-[80vh] w-full max-w-2xl flex-col overflow-hidden rounded-card border border-ui-border bg-ui-panel shadow-ui-modal animate-modal-in">
        <div className="flex items-center justify-between border-b border-ui-border px-6 py-4">
          <div>
            <h2 id={titleId} className="section-title">What&rsquo;s new</h2>
            <p className="mt-0.5 text-xs text-ui-textMuted">Version {version}</p>
          </div>
          <button
            type="button"
            onClick={onClose}
            aria-label="Close"
            className="rounded-md p-1.5 text-ui-textMuted transition-colors hover:bg-ui-sunken/60 hover:text-ui-text"
          >
            <svg className="h-5 w-5" fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden>
              <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M6 18L18 6M6 6l12 12" />
            </svg>
          </button>
        </div>

        <div className="min-h-0 flex-1 overflow-y-auto px-6 py-5 text-sm text-ui-text">
          <ReactMarkdown
            remarkPlugins={[remarkGfm]}
            components={{
              h1: ({ children }) => (
                <h1 className="mb-3 mt-6 border-b border-ui-border pb-2 text-xl font-medium text-ui-text first:mt-0">{children}</h1>
              ),
              h2: ({ children }) => (
                <h2 className="mb-2 mt-6 border-b border-ui-border pb-1.5 text-base font-medium text-ui-text first:mt-0">{children}</h2>
              ),
              h3: ({ children }) => (
                <h3 className="mb-2 mt-5 text-sm font-medium text-ui-text first:mt-0">{children}</h3>
              ),
              p: ({ children }) => <p className="mb-3 leading-relaxed text-ui-textMuted">{children}</p>,
              ul: ({ children }) => <ul className="mb-3 list-disc space-y-1 pl-5 text-ui-textMuted">{children}</ul>,
              ol: ({ children }) => <ol className="mb-3 list-decimal space-y-1 pl-5 text-ui-textMuted">{children}</ol>,
              li: ({ children }) => <li className="leading-relaxed">{children}</li>,
              strong: ({ children }) => <strong className="font-medium text-ui-text">{children}</strong>,
              em: ({ children }) => <em className="italic">{children}</em>,
              hr: () => <hr className="my-5 border-ui-border" />,
              blockquote: ({ children }) => (
                <blockquote className="mb-3 border-l-2 border-ui-accent/60 pl-3 text-ui-textMuted">{children}</blockquote>
              ),
              code: ({ children }) => (
                <code className="rounded bg-ui-sunken px-1.5 py-0.5 font-mono text-[0.85em] text-ui-text">{children}</code>
              ),
              // Wide blocks scroll inside themselves rather than stretching the modal.
              pre: ({ children }) => (
                <pre className="mb-3 overflow-x-auto rounded-panel bg-ui-sunken p-3 font-mono text-xs leading-relaxed text-ui-text">
                  {children}
                </pre>
              ),
              table: ({ children }) => (
                <div className="mb-3 overflow-x-auto">
                  <table className="w-full border-collapse text-left">{children}</table>
                </div>
              ),
              th: ({ children }) => (
                <th className="border border-ui-border px-2 py-1 font-medium text-ui-text">{children}</th>
              ),
              td: ({ children }) => (
                <td className="border border-ui-border px-2 py-1 text-ui-textMuted">{children}</td>
              ),
              img: () => null,
              a: ({ href, children }) => (
                <a
                  href={href}
                  className="text-ui-text underline decoration-ui-borderStrong underline-offset-2 hover:decoration-ui-text"
                  onClick={(e) => {
                    e.preventDefault();
                    if (href) window.electron?.openExternal?.(href);
                  }}
                >
                  {children}
                </a>
              ),
            }}
          >
            {notes}
          </ReactMarkdown>
        </div>

        <div className="flex items-center justify-between gap-3 border-t border-ui-border px-6 py-4">
          <button
            type="button"
            onClick={() => window.electron?.openExternal?.(url)}
            className="text-xs text-ui-textMuted transition-colors hover:text-ui-text"
          >
            View on GitHub
          </button>
          {onStartTour ? (
            <div className="flex items-center gap-2">
              <button
                type="button"
                onClick={onClose}
                className="btn-secondary"
              >
                Skip tour
              </button>
              <button
                type="button"
                onClick={onStartTour}
                className="btn-primary"
              >
                Start tour
              </button>
            </div>
          ) : (
            <button
              type="button"
              onClick={onClose}
              className="btn-primary"
            >
              Got it
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

export default ChangelogModal;

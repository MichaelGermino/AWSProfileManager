/**
 * AI Assistant panel: chat-like UI to generate AWS CLI examples via REST (IPC).
 * Responses include command, explanation, and "Insert Into Terminal" button.
 */

import { useState, useRef, useEffect } from 'react';
import { generateAwsCliExample } from '../api/aiClient';
import { Tooltip } from '../components/Tooltip';

export interface AIMessage {
  role: 'user' | 'assistant';
  content: string;
  command?: string;
}

interface AIAssistantPanelProps {
  onInsertCommand: (command: string) => void;
  /** When set, this prompt is sent to the AI immediately (e.g. from Command Details "Ask AI"). */
  externalPrompt?: string | null;
  /** Called after an external prompt has been sent so the parent can clear it. */
  onExternalPromptSent?: () => void;
  /** If false, show a message to configure Open WebUI in Settings instead of the chat. */
  aiConfigured?: boolean;
  /** Called when user clicks to open Settings (e.g. to configure Open WebUI). */
  onOpenSettings?: () => void;
  /** Optional content to render at the start of the header (e.g. collapse/expand button). */
  headerLeftContent?: React.ReactNode;
}

export function AIAssistantPanel({
  onInsertCommand,
  externalPrompt,
  onExternalPromptSent,
  aiConfigured = false,
  onOpenSettings,
  headerLeftContent,
}: AIAssistantPanelProps) {
  const [messages, setMessages] = useState<AIMessage[]>([]);
  const [input, setInput] = useState('');
  const [loading, setLoading] = useState(false);
  const messagesEndRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    messagesEndRef.current?.scrollIntoView({ behavior: 'smooth' });
  }, [messages]);

  const sendPrompt = async (prompt: string, isExternal = false) => {
    if (!prompt.trim() || loading) return;

    setMessages((prev) => [...prev, { role: 'user', content: prompt.trim() }]);
    setLoading(true);

    try {
      const result = await generateAwsCliExample(prompt.trim());
      setMessages((prev) => [
        ...prev,
        {
          role: 'assistant',
          content: result.explanation,
          command: result.command ? result.command.trim() : undefined,
        },
      ]);
    } catch (err) {
      setMessages((prev) => [
        ...prev,
        {
          role: 'assistant',
          content: `Error: ${err instanceof Error ? err.message : String(err)}`,
        },
      ]);
    } finally {
      setLoading(false);
      if (isExternal) onExternalPromptSent?.();
    }
  };

  useEffect(() => {
    if (aiConfigured && externalPrompt?.trim()) {
      sendPrompt(externalPrompt.trim(), true);
    }
  }, [aiConfigured, externalPrompt]);

  const sendMessage = async () => {
    const prompt = input.trim();
    if (!prompt || loading) return;

    setInput('');
    await sendPrompt(prompt);
  };

  const clearChat = () => {
    setMessages([]);
  };

  return (
    <div className="flex flex-col h-full min-h-0 bg-discord-darker/50">
      <div className="flex-shrink-0 px-3 py-2 border-b border-discord-border flex items-start justify-between gap-2">
        <div className="flex items-center gap-2 min-w-0">
          {headerLeftContent}
          <div className="min-w-0">
            <h3 className="text-sm font-semibold text-discord-text">AI Assistant</h3>
            <p className="text-xs text-discord-textMuted mt-0.5">Ask for AWS CLI examples</p>
          </div>
        </div>
        <Tooltip label="New chat" placement="left">
          <button
            type="button"
            onClick={clearChat}
            className="flex-shrink-0 p-1.5 rounded-md text-discord-textMuted hover:text-discord-text hover:bg-discord-panel transition-colors"
            aria-label="New chat"
          >
          <svg className="h-4 w-4" fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden>
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M11 5H6a2 2 0 00-2 2v11a2 2 0 002 2h11a2 2 0 002-2v-5m-1.414-9.414a2 2 0 112.828 2.828L11.828 15H9v-2.828l8.586-8.586z" />
          </svg>
          </button>
        </Tooltip>
      </div>

      <div className="flex-1 overflow-auto min-h-0 p-3 space-y-3">
        {!aiConfigured ? (
          <div className="rounded-lg p-4 bg-discord-panel border border-discord-border text-center">
            <p className="text-discord-textMuted text-sm mb-3">
              To use the AI assistant, add your Open WebUI API URL and API key in Settings.
            </p>
            <p className="text-discord-textMuted text-xs mb-4">
              Visit <strong className="text-discord-text">Open WebUI Integration</strong> in Settings to configure.
            </p>
            {onOpenSettings && (
              <button
                type="button"
                onClick={onOpenSettings}
                className="px-4 py-2 rounded-lg bg-discord-accent text-white text-sm font-medium hover:bg-discord-accentHover transition-colors"
              >
                Open Settings
              </button>
            )}
          </div>
        ) : messages.length === 0 ? (
          <p className="text-discord-textMuted text-sm">e.g. &quot;How do I list S3 buckets?&quot;</p>
        ) : null}
        {messages.map((msg, i) => (
          <div
            key={i}
            className={`rounded-lg p-3 text-sm ${
              msg.role === 'user'
                ? 'bg-discord-accent/20 text-discord-text ml-4'
                : 'bg-discord-panel text-discord-textMuted mr-4'
            }`}
          >
            <p className="whitespace-pre-wrap">{msg.content}</p>
            {msg.command && (
              <div className="mt-2">
                <pre className="p-2 rounded bg-discord-darkest border border-discord-border text-discord-text text-xs font-mono overflow-x-auto mb-2">
                  {msg.command}
                </pre>
                <button
                  type="button"
                  onClick={() => onInsertCommand(msg.command!)}
                  className="px-2 py-1 rounded text-xs font-medium bg-discord-accent text-white hover:bg-discord-accentHover transition-colors"
                >
                  Insert Into Terminal
                </button>
              </div>
            )}
          </div>
        ))}
        {aiConfigured && loading && (
          <div className="rounded-lg p-3 bg-discord-panel flex items-center gap-2 ai-thinking-stars" aria-label="Thinking">
            <span>
              <svg className="h-5 w-5 text-discord-accent" fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden>
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M5 3v4M3 5h4M6 17v4m-2-2h4m5-16l2.286 6.857L21 12l-5.714 2.143L13 21l-2.286-6.857L5 12l5.714-2.143L13 3z" />
              </svg>
            </span>
            <span>
              <svg className="h-5 w-5 text-discord-accent" fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden>
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M5 3v4M3 5h4M6 17v4m-2-2h4m5-16l2.286 6.857L21 12l-5.714 2.143L13 21l-2.286-6.857L5 12l5.714-2.143L13 3z" />
              </svg>
            </span>
            <span>
              <svg className="h-5 w-5 text-discord-accent" fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden>
                <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M5 3v4M3 5h4M6 17v4m-2-2h4m5-16l2.286 6.857L21 12l-5.714 2.143L13 21l-2.286-6.857L5 12l5.714-2.143L13 3z" />
              </svg>
            </span>
          </div>
        )}
        <div ref={messagesEndRef} />
      </div>

      {aiConfigured && (
        <div className="flex-shrink-0 p-3 border-t border-discord-border flex gap-2">
          <input
            type="text"
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && !e.shiftKey && sendMessage()}
            placeholder="Ask for an AWS CLI command…"
            className="flex-1 min-w-0 px-3 py-2 rounded-lg bg-discord-darkest border border-discord-border text-discord-text text-sm placeholder-discord-textMuted focus:border-discord-accent focus:ring-1 focus:ring-discord-accent"
            disabled={loading}
            aria-label="AI prompt"
          />
          <button
            type="button"
            onClick={sendMessage}
            disabled={loading || !input.trim()}
            className="px-4 py-2 rounded-lg bg-discord-accent text-white text-sm font-medium hover:bg-discord-accentHover disabled:opacity-50 disabled:cursor-not-allowed transition-colors"
          >
            Send
          </button>
        </div>
      )}
    </div>
  );
}

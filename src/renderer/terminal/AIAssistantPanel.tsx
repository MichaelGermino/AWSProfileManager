/**
 * AI Assistant panel: multi-turn chat for AWS CLI help via REST (IPC).
 *
 * The whole thread is sent on every turn, so follow-up questions work. Assistant replies
 * are Markdown and render via AIMarkdown, which puts an "Insert Into Terminal" button on
 * each runnable code block.
 */

import { useState, useRef, useEffect } from 'react';
import { sendAiChatStream, abortAiChat } from '../api/aiClient';
import type { AiChatMessage } from '../api/aiClient';
import { AIMarkdown } from './AIMarkdown';
import { Tooltip } from '../components/Tooltip';

/** One-click follow-ups, shown after a successful reply. They read as continuations of the
 *  thread, which only works because the whole conversation is now sent on each turn. */
const FOLLOW_UP_PROMPTS = ['Show more options', 'Give another example', 'What could go wrong?'];

/** Treat "within this many px of the bottom" as parked at the bottom. Covers sub-pixel
 *  rounding and a token landing between the scroll event and the re-render. */
const BOTTOM_PIN_THRESHOLD_PX = 48;

export interface AIMessage {
  role: 'user' | 'assistant';
  content: string;
  /** True when content is a local/API error rather than a model reply; not sent back as history. */
  isError?: boolean;
}

interface AIAssistantPanelProps {
  onInsertCommand: (command: string) => void;
  /** When set, this prompt is sent to the AI immediately (e.g. from Command details "Ask AI"). */
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
  /** Text accumulated so far for the in-flight reply; null when nothing is streaming. */
  const [streamingText, setStreamingText] = useState<string | null>(null);
  const activeRequestRef = useRef<string | null>(null);
  const scrollRef = useRef<HTMLDivElement>(null);
  /** Whether the view is parked at the bottom. False once the user scrolls up to read
   *  back, which suppresses auto-scroll until they return to the bottom themselves. */
  const pinnedToBottomRef = useRef(true);

  const handleScroll = () => {
    const el = scrollRef.current;
    if (!el) return;
    const distanceFromBottom = el.scrollHeight - el.scrollTop - el.clientHeight;
    pinnedToBottomRef.current = distanceFromBottom <= BOTTOM_PIN_THRESHOLD_PX;
  };

  useEffect(() => {
    if (!pinnedToBottomRef.current) return;
    const el = scrollRef.current;
    if (!el) return;
    // Jump rather than smooth-scroll: this fires on every token, and an in-flight smooth
    // animation would sit far from the bottom when the next scroll event lands, flipping
    // pinnedToBottomRef to false and cancelling the very behaviour we want.
    el.scrollTop = el.scrollHeight;
  }, [messages, streamingText]);

  const sendPrompt = async (prompt: string, isExternal = false) => {
    if (!prompt.trim() || loading) return;

    // Build the outgoing thread from current state plus this turn. Using the local
    // array (rather than reading `messages` after setState) avoids sending a stale
    // history, and error bubbles are dropped so failures don't poison the context.
    const nextMessages: AIMessage[] = [...messages, { role: 'user', content: prompt.trim() }];
    const history: AiChatMessage[] = nextMessages
      .filter((m) => !m.isError)
      .map((m) => ({ role: m.role, content: m.content }));

    const requestId =
      globalThis.crypto?.randomUUID?.() ?? `ai-${Date.now()}-${Math.random().toString(16).slice(2)}`;
    activeRequestRef.current = requestId;

    // Sending is an explicit "show me what happens next", so re-pin even if the user had
    // scrolled up to re-read something earlier in the thread.
    pinnedToBottomRef.current = true;

    setMessages(nextMessages);
    setLoading(true);
    setStreamingText('');

    try {
      const result = await sendAiChatStream(requestId, history, (delta) => {
        setStreamingText((prev) => (prev ?? '') + delta);
      });

      // A stopped stream keeps whatever arrived — a partial answer is still useful.
      const stoppedEmpty = result.aborted && !result.content.trim();
      setMessages((prev) => [
        ...prev,
        {
          role: 'assistant',
          content: stoppedEmpty ? 'Stopped before any response arrived.' : result.content,
          isError: result.isError || stoppedEmpty,
        },
      ]);
    } catch (err) {
      setMessages((prev) => [
        ...prev,
        {
          role: 'assistant',
          content: `Error: ${err instanceof Error ? err.message : String(err)}`,
          isError: true,
        },
      ]);
    } finally {
      setLoading(false);
      setStreamingText(null);
      activeRequestRef.current = null;
      if (isExternal) onExternalPromptSent?.();
    }
  };

  const stopStreaming = () => {
    if (activeRequestRef.current) abortAiChat(activeRequestRef.current);
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
    <div className="flex flex-col h-full min-h-0 bg-ui-panel">
      <div className="flex-shrink-0 px-3 py-2 border-b border-ui-border flex items-start justify-between gap-2">
        <div className="flex items-center gap-2 min-w-0">
          {headerLeftContent}
          <div className="min-w-0">
            <h3 className="text-sm font-medium text-ui-text">AI assistant</h3>
            <p className="text-xs text-ui-textMuted mt-0.5">Ask for AWS CLI examples</p>
          </div>
        </div>
        <Tooltip label="New chat" placement="left">
          <button
            type="button"
            onClick={clearChat}
            className="btn-icon"
            aria-label="New chat"
          >
          <svg className="h-4 w-4" fill="none" stroke="currentColor" viewBox="0 0 24 24" aria-hidden>
            <path strokeLinecap="round" strokeLinejoin="round" strokeWidth={2} d="M11 5H6a2 2 0 00-2 2v11a2 2 0 002 2h11a2 2 0 002-2v-5m-1.414-9.414a2 2 0 112.828 2.828L11.828 15H9v-2.828l8.586-8.586z" />
          </svg>
          </button>
        </Tooltip>
      </div>

      <div ref={scrollRef} onScroll={handleScroll} className="flex-1 overflow-auto min-h-0 p-3 space-y-3">
        {!aiConfigured ? (
          <div className="rounded-lg p-4 bg-ui-panel border border-ui-border text-center">
            <p className="text-ui-textMuted text-sm mb-3">
              To use the AI assistant, add your Open WebUI API URL and API key in Settings.
            </p>
            <p className="text-ui-textMuted text-xs mb-4">
              Set it up in Settings, under Terminal &amp; AI.
            </p>
            {onOpenSettings && (
              <button
                type="button"
                onClick={onOpenSettings}
                className="btn-primary"
              >
                Open Settings
              </button>
            )}
          </div>
        ) : messages.length === 0 ? (
          <p className="text-ui-textMuted text-sm">e.g. &quot;How do I list S3 buckets?&quot;</p>
        ) : null}
        {messages.map((msg, i) => (
          <div
            key={i}
            className={`rounded-card px-3 py-2 text-sm ${
 msg.role === 'user'
 ? 'bg-ui-raised text-ui-text ml-6'
 : msg.isError
 ? 'border border-ui-danger/40 text-ui-text mr-6'
 : 'text-ui-text'
 }`}
          >
            {msg.role === 'assistant' && !msg.isError ? (
              <AIMarkdown content={msg.content} onInsertCommand={onInsertCommand} />
            ) : (
              <p className="whitespace-pre-wrap">{msg.content}</p>
            )}
          </div>
        ))}
        {aiConfigured && loading && streamingText ? (
          // Stop lives in the composer, not here: this bubble grows as tokens arrive, so a
          // button underneath it would slide down the screen as you reached for it.
          <div className="rounded-card px-3 py-2 text-sm text-ui-text">
            <AIMarkdown content={streamingText} onInsertCommand={onInsertCommand} />
          </div>
        ) : null}

        {aiConfigured && loading && !streamingText && (
          <div className="ai-thinking mr-4 flex items-center gap-1 rounded-card bg-ui-raised px-3 py-3" role="status" aria-label="Thinking">
            <span />
            <span />
            <span />
          </div>
        )}
        {aiConfigured &&
          !loading &&
          messages.length > 0 &&
          messages[messages.length - 1].role === 'assistant' &&
          !messages[messages.length - 1].isError && (
            <div className="flex flex-wrap gap-1.5 mr-4">
              {FOLLOW_UP_PROMPTS.map((prompt) => (
                <button
                  key={prompt}
                  type="button"
                  onClick={() => sendPrompt(prompt)}
                  className="btn-secondary btn-sm h-7 px-2"
                >
                  {prompt}
                </button>
              ))}
            </div>
          )}
      </div>

      {aiConfigured && (
        <div className="flex-shrink-0 p-3 border-t border-ui-border flex gap-2">
          <input
            type="text"
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && !e.shiftKey && sendMessage()}
            placeholder="Ask for an AWS CLI command…"
            className="input flex-1 min-w-0"
            disabled={loading}
            aria-label="AI prompt"
          />
          {loading ? (
            <button
              type="button"
              onClick={stopStreaming}
              className="btn-secondary"
            >
              Stop
            </button>
          ) : (
            <button
              type="button"
              onClick={sendMessage}
              disabled={!input.trim()}
              className="btn-primary"
            >
              Send
            </button>
          )}
        </div>
      )}
    </div>
  );
}

/**
 * Renderer-side client for the AI assistant.
 * All requests go via IPC to the main process; the API key is never exposed.
 */

export interface AiChatMessage {
  role: 'user' | 'assistant';
  content: string;
}

export interface AiChatResponse {
  /** Assistant reply as GitHub-flavoured Markdown, or an error message to show in the thread. */
  content: string;
  isError?: boolean;
  /** True when the user pressed Stop; content holds whatever arrived before that. */
  aborted?: boolean;
}

/**
 * Send the conversation so far and get the next assistant reply.
 * Passing the whole thread (not just the latest prompt) is what makes follow-up
 * questions work — the model needs the earlier turns to refer back to them.
 */
export async function sendAiChat(messages: AiChatMessage[]): Promise<AiChatResponse> {
  if (!window.electron?.aiChat) {
    return { content: 'Terminal AI is not available (missing IPC).', isError: true };
  }
  return window.electron.aiChat({ messages });
}

/**
 * Streaming send. `onDelta` fires per token; the promise resolves with the full reply.
 * Falls back to the blocking call when the streaming IPC isn't available.
 */
export async function sendAiChatStream(
  requestId: string,
  messages: AiChatMessage[],
  onDelta: (delta: string) => void
): Promise<AiChatResponse> {
  if (!window.electron?.aiChatStream || !window.electron?.onAiChatChunk) {
    return sendAiChat(messages);
  }

  const unsubscribe = window.electron.onAiChatChunk((payload) => {
    if (payload.requestId === requestId) onDelta(payload.delta);
  });

  try {
    return await window.electron.aiChatStream({ requestId, messages });
  } finally {
    unsubscribe();
  }
}

/** Ask main to abort an in-flight stream (Stop button). */
export function abortAiChat(requestId: string): void {
  void window.electron?.aiChatAbort?.(requestId);
}

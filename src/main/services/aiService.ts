/**
 * AI service: calls Open WebUI chat/completions API to generate AWS CLI examples.
 * API key and URL are configured here and must NEVER be exposed to the renderer.
 * @see https://openwebui.com/ (Open WebUI uses OpenAI-compatible API)
 */

import { getSettings } from './settingsService';
import { getHttpUserAgent } from './enterpriseTls';

export interface AiChatMessage {
  role: 'user' | 'assistant';
  content: string;
}

export interface AiChatResult {
  /** Assistant reply as GitHub-flavoured Markdown, or an error message to show in the thread. */
  content: string;
  /** True when content is an error rather than a model reply. */
  isError?: boolean;
  /** True when the user stopped generation; content holds whatever arrived first. */
  aborted?: boolean;
}

/**
 * How many prior messages to send. Poppy prepends its own ~1,200-token preamble to every
 * call, so unbounded history would creep toward the context limit over a long session.
 * Trimming from the start keeps the most recent turns, which is what follow-ups reference.
 */
const MAX_HISTORY_MESSAGES = 20;

/**
 * Headers common to every Open WebUI call.
 *
 * The explicit User-Agent is load-bearing: Node's fetch() otherwise sends
 * `User-Agent: node`, which some API edges (poppy.ca.gov among them) treat as a bot
 * and reject with a bare HTML 403 before Open WebUI ever sees the request. The undici
 * dispatcher in enterpriseTls.ts also rewrites that default, but this makes the
 * requirement visible at the call site and holds even if the dispatcher isn't installed.
 */
function baseHeaders(apiKey: string): Record<string, string> {
  return {
    Accept: 'application/json',
    Authorization: `Bearer ${apiKey}`,
    'User-Agent': getHttpUserAgent(),
  };
}

/**
 * Render a non-OK response into something actionable. An HTML body means a proxy, WAF,
 * or bot filter answered instead of Open WebUI — a very different problem from a
 * JSON error, and worth distinguishing since the raw HTML tells the user nothing.
 */
function describeHttpError(status: number, body: string): string {
  const trimmed = body.trim();
  if (/^<(!doctype|html)/i.test(trimmed)) {
    return `API error (${status}): request was blocked before it reached Open WebUI (an HTML error page came back instead of JSON). Verify the API URL, and check whether this network allows non-browser API clients.`;
  }
  return `API error (${status}): ${trimmed.slice(0, 200)}`;
}

/**
 * The previous prompt demanded a single JSON object with a one-sentence explanation,
 * which structurally could not contain options or examples — the reason "Ask AI how to
 * use this" gave such thin answers. Markdown is rendered by the panel, so ask for it.
 */
const AWS_CLI_SYSTEM_PROMPT = `You are an expert AWS CLI assistant embedded in a desktop app, helping the user work in an AWS terminal.

Answer in GitHub-flavoured Markdown. Be concrete and practical:
- Explain what the command does, then cover the options that actually matter.
- Give at least two worked examples using realistic values.
- Call out destructive or irreversible behaviour explicitly.
- Put every runnable command in its own \`\`\`bash fenced code block, one command per block, so it can be inserted into the terminal as-is.
- Keep prose tight; prefer short paragraphs, lists and headings over long text.

This is a conversation — the user can ask follow-up questions, so refer back to earlier turns naturally instead of repeating yourself.`;

/**
 * Call Open WebUI chat/completions with the full conversation and return the assistant's
 * Markdown reply. All requests run in the main process so the API key is never exposed.
 */
export async function chatWithAi(messages: AiChatMessage[]): Promise<AiChatResult> {
  const request = buildChatRequest(messages, false);
  if ('error' in request) return request.error;

  const response = await fetch(request.url, {
    method: 'POST',
    headers: request.headers,
    body: JSON.stringify(request.body),
  });

  if (!response.ok) {
    const text = await response.text();
    return { content: describeHttpError(response.status, text), isError: true };
  }

  const data = (await response.json()) as {
    choices?: Array<{ message?: { content?: string | null }; finish_reason?: string }>;
  };

  const choice = data.choices?.[0];
  const content = choice?.message?.content ?? '';

  if (!content.trim()) {
    // Don't fail silently: a null content with a non-"stop" finish reason means the model
    // did something we don't handle (a tool call, a filter), and a blank bubble is baffling.
    const reason = choice?.finish_reason ? ` (finish_reason: ${choice.finish_reason})` : '';
    return { content: `The model returned an empty response${reason}.`, isError: true };
  }

  return { content };
}

/** Shared request construction for the streaming and non-streaming paths. */
function buildChatRequest(
  messages: AiChatMessage[],
  stream: boolean
): { url: string; headers: Record<string, string>; body: unknown } | { error: AiChatResult } {
  const settings = getSettings();
  const baseUrl = (settings.openWebUiApiUrl ?? '').trim().replace(/\/$/, '');
  const apiKey = (settings.openWebUiApiKey ?? '').trim();
  const model = (settings.openWebUiModel ?? '').trim();

  if (!baseUrl || !apiKey) {
    throw new Error('Open WebUI is not configured. Add your API URL and API key in Settings → Open WebUI Integration.');
  }

  // No hardcoded fallback model: model ids are instance-specific, so a baked-in default
  // silently rots the moment the endpoint changes and surfaces as an opaque
  // 400 "Model not found". Make the user pick one that the instance actually offers.
  if (!model) {
    throw new Error('No Open WebUI model selected. Choose one in Settings → Open WebUI Integration.');
  }

  const history = messages
    .filter((m) => typeof m.content === 'string' && m.content.trim())
    .slice(-MAX_HISTORY_MESSAGES)
    .map((m) => ({ role: m.role, content: m.content }));

  if (history.length === 0) {
    return { error: { content: 'Nothing to send.', isError: true } };
  }

  return {
    url: `${baseUrl}/chat/completions`,
    headers: { ...baseHeaders(apiKey), 'Content-Type': 'application/json' },
    body: {
      model,
      messages: [{ role: 'system' as const, content: AWS_CLI_SYSTEM_PROMPT }, ...history],
      // This instance attaches server-side tools. Left to itself the model may answer with
      // finish_reason "tool_calls" and content:null, which renders as an empty reply.
      // Declining tools also drops the injected tool definitions (~1400 prompt tokens).
      tool_choice: 'none' as const,
      ...(stream ? { stream: true } : {}),
    },
  };
}

/**
 * Streaming variant: calls onDelta with each token as it arrives and resolves with the
 * full text. Long answers (3KB+ is typical now) otherwise sit behind a spinner for many
 * seconds with nothing to read.
 *
 * Abort via the signal resolves with whatever text arrived, flagged `aborted` — a partial
 * answer is still useful and stays in the thread.
 */
export async function streamChatWithAi(
  messages: AiChatMessage[],
  signal: AbortSignal,
  onDelta: (delta: string) => void
): Promise<AiChatResult> {
  const request = buildChatRequest(messages, true);
  if ('error' in request) return request.error;

  let response: Response;
  try {
    response = await fetch(request.url, {
      method: 'POST',
      headers: { ...request.headers, Accept: 'text/event-stream' },
      body: JSON.stringify(request.body),
      signal,
    });
  } catch (err) {
    if (signal.aborted) return { content: '', aborted: true };
    throw err;
  }

  if (!response.ok) {
    const text = await response.text();
    return { content: describeHttpError(response.status, text), isError: true };
  }

  // If the server ignored `stream` and answered with a normal JSON completion, don't try
  // to parse it as SSE — fall back to reading it whole.
  const contentType = response.headers.get('content-type') ?? '';
  if (!contentType.includes('text/event-stream')) {
    const data = (await response.json()) as {
      choices?: Array<{ message?: { content?: string | null }; finish_reason?: string }>;
    };
    const whole = data.choices?.[0]?.message?.content ?? '';
    if (whole) onDelta(whole);
    return whole.trim()
      ? { content: whole }
      : { content: 'The model returned an empty response.', isError: true };
  }

  const reader = response.body?.getReader();
  if (!reader) return { content: 'The model returned no response body.', isError: true };

  const decoder = new TextDecoder();
  let buffer = '';
  let full = '';
  let finishReason = '';

  try {
    for (;;) {
      const { done, value } = await reader.read();
      if (done) break;

      buffer += decoder.decode(value, { stream: true });

      // SSE frames are separated by a blank line; keep the trailing partial in the buffer.
      const frames = buffer.split('\n\n');
      buffer = frames.pop() ?? '';

      for (const frame of frames) {
        for (const line of frame.split('\n')) {
          if (!line.startsWith('data:')) continue;
          const payload = line.slice(5).trim();
          if (!payload || payload === '[DONE]') continue;

          try {
            const parsed = JSON.parse(payload) as {
              choices?: Array<{ delta?: { content?: string | null }; finish_reason?: string | null }>;
            };
            const choice = parsed.choices?.[0];
            if (choice?.finish_reason) finishReason = choice.finish_reason;
            const delta = choice?.delta?.content;
            if (delta) {
              full += delta;
              onDelta(delta);
            }
          } catch {
            // A malformed frame shouldn't kill the stream; skip it and keep reading.
          }
        }
      }
    }
  } catch (err) {
    if (signal.aborted) return { content: full, aborted: true };
    throw err;
  } finally {
    reader.releaseLock();
  }

  if (signal.aborted) return { content: full, aborted: true };

  if (!full.trim()) {
    const reason = finishReason ? ` (finish_reason: ${finishReason})` : '';
    return { content: `The model returned an empty response${reason}.`, isError: true };
  }

  return { content: full };
}

/** Returns whether Open WebUI is configured (URL and API key set). Safe to call from IPC; no secrets. */
export function getOpenWebUiConfigStatus(): { configured: boolean } {
  const settings = getSettings();
  const baseUrl = (settings.openWebUiApiUrl ?? '').trim();
  const apiKey = (settings.openWebUiApiKey ?? '').trim();
  return { configured: !!baseUrl && !!apiKey };
}

/**
 * Fetch model list from Open WebUI GET /api/models. Uses URL and key from settings.
 * Returns model ids only; safe to send to renderer.
 */
export async function fetchOpenWebUiModels(): Promise<{ models: string[] } | { error: string }> {
  const settings = getSettings();
  const baseUrl = (settings.openWebUiApiUrl ?? '').trim().replace(/\/$/, '');
  const apiKey = (settings.openWebUiApiKey ?? '').trim();

  if (!baseUrl || !apiKey) {
    return { error: 'Add API URL and API key, then try again.' };
  }

  const url = `${baseUrl}/models`;
  const headers: Record<string, string> = baseHeaders(apiKey);

  try {
    const response = await fetch(url, { method: 'GET', headers });

    if (!response.ok) {
      const text = await response.text();
      return { error: describeHttpError(response.status, text) };
    }

    const data = (await response.json()) as unknown;
    const ids: string[] = [];

    if (Array.isArray(data)) {
      for (const item of data) {
        if (item && typeof item === 'object' && typeof (item as { id?: string }).id === 'string') {
          ids.push((item as { id: string }).id);
        }
      }
    } else if (data && typeof data === 'object' && Array.isArray((data as { data?: unknown[] }).data)) {
      const arr = (data as { data: unknown[] }).data;
      for (const item of arr) {
        if (item && typeof item === 'object' && typeof (item as { id?: string }).id === 'string') {
          ids.push((item as { id: string }).id);
        }
      }
    }

    return { models: [...new Set(ids)].sort((a, b) => a.localeCompare(b)) };
  } catch (err) {
    const message = err instanceof Error ? err.message : String(err);
    return { error: message };
  }
}

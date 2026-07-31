/**
 * AI service: calls Open WebUI chat/completions API to generate AWS CLI examples.
 * API key and URL are configured here and must NEVER be exposed to the renderer.
 * @see https://openwebui.com/ (Open WebUI uses OpenAI-compatible API)
 */

import { getSettings } from './settingsService';
import { getHttpUserAgent } from './enterpriseTls';

export interface GenerateCliResult {
  command: string;
  explanation: string;
}

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

const AWS_CLI_SYSTEM_PROMPT = `You are an expert in AWS CLI. For each user request, respond with a valid AWS CLI command and a brief explanation.
Reply with a single JSON object only, no other text or markdown. Use this exact format:
{"command":"aws <service> <subcommand> ...","explanation":"One sentence describing what the command does."}
The "command" must be a single line, runnable as-is (e.g. aws s3 ls, aws sts get-caller-identity --profile myprofile).`;

/**
 * Parse assistant content into command + explanation.
 * Prefer JSON shape; fallback: extract first fenced code block as command, rest as explanation.
 */
function parseResponse(content: string): GenerateCliResult {
  const trimmed = content.trim();

  // Try JSON first (e.g. {"command":"aws s3 ls","explanation":"Lists buckets."})
  const jsonMatch = trimmed.match(/\{[\s\S]*"command"[\s\S]*"explanation"[\s\S]*\}/);
  if (jsonMatch) {
    try {
      const parsed = JSON.parse(jsonMatch[0]) as { command?: string; explanation?: string };
      return {
        command: typeof parsed.command === 'string' ? parsed.command.trim() : '',
        explanation: typeof parsed.explanation === 'string' ? parsed.explanation.trim() : trimmed,
      };
    } catch {
      // fall through to fallback
    }
  }

  // Fallback: first ```...``` block as command, rest as explanation
  const codeBlockMatch = trimmed.match(/```(?:bash|sh|shell)?\s*\n?([\s\S]*?)```/);
  if (codeBlockMatch) {
    const command = codeBlockMatch[1].trim();
    const explanation = trimmed.replace(codeBlockMatch[0], '').trim() || 'AWS CLI example.';
    return { command, explanation };
  }

  // No structure: whole response as explanation, no command
  return { command: '', explanation: trimmed || 'No response.' };
}

/**
 * Call Open WebUI chat/completions with the user prompt and return a structured CLI example + explanation.
 * All requests run in the main process so the API key is never sent to the renderer.
 */
export async function generateAwsCliExample(prompt: string): Promise<GenerateCliResult> {
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

  const chatCompletionsUrl = `${baseUrl}/chat/completions`;
  const headers: Record<string, string> = {
    ...baseHeaders(apiKey),
    'Content-Type': 'application/json',
  };

  const body = {
    model,
    messages: [
      { role: 'system' as const, content: AWS_CLI_SYSTEM_PROMPT },
      { role: 'user' as const, content: prompt },
    ],
  };

  const response = await fetch(chatCompletionsUrl, {
    method: 'POST',
    headers,
    body: JSON.stringify(body),
  });

  if (!response.ok) {
    const text = await response.text();
    return {
      command: '',
      explanation: describeHttpError(response.status, text),
    };
  }

  const data = (await response.json()) as {
    choices?: Array<{ message?: { content?: string } }>;
  };
  const content =
    data.choices?.[0]?.message?.content ?? '';
  return parseResponse(content);
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

//! AI service: calls Open WebUI chat/completions API to generate AWS CLI examples.
//! Port of `aiService.ts`. API key and URL are read here and must NEVER be exposed to the window.
//! @see https://openwebui.com/ (Open WebUI uses an OpenAI-compatible API)
//!
//! Errors: `Err(message)` is what the 1.x function threw (`err.message`); results that 1.x
//! returned (including error bubbles with `isError`) are `Ok`.

use futures_util::StreamExt;
use serde_json::{json, Map, Value};
use tokio_util::sync::CancellationToken;

use super::collate::locale_compare;
use crate::core::Core;
use crate::js;
use crate::json as jsjson;

/// How many prior messages to send. Poppy prepends its own ~1,200-token preamble to every
/// call, so unbounded history would creep toward the context limit over a long session.
/// Trimming from the start keeps the most recent turns, which is what follow-ups reference.
const MAX_HISTORY_MESSAGES: usize = 20;

/// The previous prompt demanded a single JSON object with a one-sentence explanation,
/// which structurally could not contain options or examples — the reason "Ask AI how to
/// use this" gave such thin answers. Markdown is rendered by the panel, so ask for it.
const AWS_CLI_SYSTEM_PROMPT: &str = "You are an expert AWS CLI assistant embedded in a desktop app, helping the user work in an AWS terminal.

Answer in GitHub-flavoured Markdown. Be concrete and practical:
- Explain what the command does, then cover the options that actually matter.
- Give at least two worked examples using realistic values.
- Call out destructive or irreversible behaviour explicitly.
- Put every runnable command in its own ```bash fenced code block, one command per block, so it can be inserted into the terminal as-is.
- Keep prose tight; prefer short paragraphs, lists and headings over long text.

This is a conversation — the user can ask follow-up questions, so refer back to earlier turns naturally instead of repeating yourself.";

/// `{ content }`, `{ content, isError: true }` or `{ content, aborted: true }` (AiChatResult).
pub(crate) fn reply(content: &str) -> Value {
    json!({ "content": content })
}

pub(crate) fn error_reply(content: &str) -> Value {
    json!({ "content": content, "isError": true })
}

fn aborted_reply(content: &str) -> Value {
    json!({ "content": content, "aborted": true })
}

/// Node's `fetch` rejects with these messages; the 1.x handlers pass them through as-is
/// (`AI request failed: fetch failed`, `{ error: 'fetch failed' }`).
fn fetch_error(err: &reqwest::Error, url: &str) -> String {
    if err.is_builder() {
        format!("Failed to parse URL from {url}")
    } else {
        "fetch failed".to_string()
    }
}

/// Reading a body that broke off mid-way: undici's message.
const BODY_TERMINATED: &str = "terminated";
/// Reading a body after the AbortSignal fired.
const ABORTED: &str = "This operation was aborted";

/// `JSON.parse(text)` for `response.json()`: V8-style message on failure.
fn parse_json_body(text: &str) -> Result<Value, String> {
    serde_json::from_str(text).map_err(|e| {
        if js::trim(text).is_empty() || e.is_eof() {
            return "Unexpected end of JSON input".to_string();
        }
        // Character at the failing position (serde reports 1-based line/column).
        let line = text.lines().nth(e.line().saturating_sub(1)).unwrap_or("");
        let token = line.chars().nth(e.column().saturating_sub(1)).or_else(|| text.trim_start().chars().next()).unwrap_or(' ');
        let snippet: String = text.chars().take(10).collect();
        let more = if text.chars().count() > 10 { "..." } else { "" };
        format!("Unexpected token '{token}', \"{snippet}\"{more} is not valid JSON")
    })
}

/// `str.slice(0, n)` counting UTF-16 code units like JavaScript (never splits a character).
fn slice_utf16(s: &str, n: usize) -> &str {
    let mut units = 0;
    for (i, c) in s.char_indices() {
        units += c.len_utf16();
        if units > n {
            return &s[..i];
        }
    }
    s
}

/// Render a non-OK response into something actionable. An HTML body means a proxy, WAF,
/// or bot filter answered instead of Open WebUI — a very different problem from a
/// JSON error, and worth distinguishing since the raw HTML tells the user nothing.
fn describe_http_error(status: u16, body: &str) -> String {
    let trimmed = js::trim(body);
    let lower: String = trimmed.chars().take(9).collect::<String>().to_lowercase();
    if lower.starts_with("<!doctype") || lower.starts_with("<html") {
        return format!(
            "API error ({status}): request was blocked before it reached Open WebUI (an HTML error page came back instead of JSON). Verify the API URL, and check whether this network allows non-browser API clients."
        );
    }
    format!("API error ({status}): {}", slice_utf16(trimmed, 200))
}

/// `(settings.x ?? '').trim()`.
fn setting(settings: &Map<String, Value>, key: &str) -> String {
    js::trim(settings.get(key).and_then(Value::as_str).unwrap_or("")).to_string()
}

/// `.trim().replace(/\/$/, '')`: one trailing slash.
fn base_url(settings: &Map<String, Value>) -> String {
    let url = setting(settings, "openWebUiApiUrl");
    url.strip_suffix('/').map(str::to_string).unwrap_or(url)
}

struct ChatRequest {
    url: String,
    api_key: String,
    body: Value,
}

/// `choices[0]` of a completion or stream frame (`data.choices?.[0]`).
fn first_choice(v: &Value) -> Option<&Value> {
    v.get("choices").and_then(|c| c.get(0))
}

/// The JavaScript string of a truthy `finish_reason`, else `None`.
fn finish_reason(choice: Option<&Value>) -> Option<String> {
    match choice?.get("finish_reason")? {
        Value::String(s) if !s.is_empty() => Some(s.clone()),
        Value::Number(n) if n.as_f64() != Some(0.0) => Some(n.to_string()),
        Value::Bool(true) => Some("true".into()),
        v @ (Value::Array(_) | Value::Object(_)) => Some(jsjson::stringify(v)),
        _ => None,
    }
}

/// `message.content ?? ''` of a non-streamed completion.
fn message_content(choice: Option<&Value>) -> String {
    choice.and_then(|c| c.get("message")).and_then(|m| m.get("content")).and_then(Value::as_str).unwrap_or("").to_string()
}

impl Core {
    fn ai_request(&self, url: &str, api_key: &str, post: bool, accept: &str) -> reqwest::RequestBuilder {
        let client = self.cfg.http.client(false, true);
        let target = self.cfg.http.remap(url);
        let b = if post { client.post(target) } else { client.get(target) };
        // Headers common to every Open WebUI call.
        //
        // The explicit User-Agent is load-bearing: Node's fetch() otherwise sends
        // `User-Agent: node`, which some API edges (poppy.ca.gov among them) treat as a bot
        // and reject with a bare HTML 403 before Open WebUI ever sees the request. The shared
        // client already sets it; repeating it keeps the requirement visible at the call site.
        b.header("Accept", accept).header("Authorization", format!("Bearer {api_key}")).header("User-Agent", self.cfg.http.user_agent.clone())
    }

    /// Shared request construction for the streaming and non-streaming paths.
    /// `Ok(Err(reply))` is a result to return without calling the server.
    fn build_chat_request(&self, messages: &Value, stream: bool) -> Result<Result<ChatRequest, Value>, String> {
        let settings = self.settings().get();
        let base = base_url(&settings);
        let api_key = setting(&settings, "openWebUiApiKey");
        let model = setting(&settings, "openWebUiModel");

        if base.is_empty() || api_key.is_empty() {
            return Err("Open WebUI is not configured. Add your API URL and API key in Settings → Open WebUI Integration.".into());
        }

        // No hardcoded fallback model: model ids are instance-specific, so a baked-in default
        // silently rots the moment the endpoint changes and surfaces as an opaque
        // 400 "Model not found". Make the user pick one that the instance actually offers.
        if model.is_empty() {
            return Err("No Open WebUI model selected. Choose one in Settings → Open WebUI Integration.".into());
        }

        let all: Vec<Value> = messages
            .as_array()
            .map(|list| {
                list.iter()
                    .filter(|m| m.get("content").and_then(Value::as_str).is_some_and(|c| !js::trim(c).is_empty()))
                    .map(|m| {
                        let mut out = Map::new();
                        // `{ role: m.role, content }`: an undefined role is dropped by JSON.stringify.
                        if let Some(role) = m.get("role") {
                            out.insert("role".into(), role.clone());
                        }
                        out.insert("content".into(), m["content"].clone());
                        Value::Object(out)
                    })
                    .collect()
            })
            .unwrap_or_default();
        let history = all[all.len().saturating_sub(MAX_HISTORY_MESSAGES)..].to_vec();

        if history.is_empty() {
            return Ok(Err(error_reply("Nothing to send.")));
        }

        let mut msgs = vec![json!({ "role": "system", "content": AWS_CLI_SYSTEM_PROMPT })];
        msgs.extend(history);
        let mut body = Map::new();
        body.insert("model".into(), Value::String(model));
        body.insert("messages".into(), Value::Array(msgs));
        // This instance attaches server-side tools. Left to itself the model may answer with
        // finish_reason "tool_calls" and content:null, which renders as an empty reply.
        // Declining tools also drops the injected tool definitions (~1400 prompt tokens).
        body.insert("tool_choice".into(), Value::String("none".into()));
        if stream {
            body.insert("stream".into(), Value::Bool(true));
        }
        Ok(Ok(ChatRequest { url: format!("{base}/chat/completions"), api_key, body: Value::Object(body) }))
    }

    /// Call Open WebUI chat/completions with the full conversation and return the assistant's
    /// Markdown reply. All requests run in the core so the API key is never exposed.
    pub async fn chat_with_ai(&self, messages: &Value) -> Result<Value, String> {
        let request = match self.build_chat_request(messages, false)? {
            Ok(r) => r,
            Err(result) => return Ok(result),
        };

        let response = self
            .ai_request(&request.url, &request.api_key, true, "application/json")
            .header("Content-Type", "application/json")
            .body(jsjson::stringify(&request.body))
            .send()
            .await
            .map_err(|e| fetch_error(&e, &request.url))?;

        let status = response.status();
        let text = response.text().await.map_err(|_| BODY_TERMINATED.to_string())?;
        if !status.is_success() {
            return Ok(error_reply(&describe_http_error(status.as_u16(), &text)));
        }

        let data = parse_json_body(&text)?;
        let choice = first_choice(&data);
        let content = message_content(choice);

        if js::trim(&content).is_empty() {
            // Don't fail silently: a null content with a non-"stop" finish reason means the model
            // did something we don't handle (a tool call, a filter), and a blank bubble is baffling.
            let reason = finish_reason(choice).map(|r| format!(" (finish_reason: {r})")).unwrap_or_default();
            return Ok(error_reply(&format!("The model returned an empty response{reason}.")));
        }

        Ok(reply(&content))
    }

    /// Streaming variant: calls `on_delta` with each token as it arrives and resolves with the
    /// full text. Long answers (3KB+ is typical now) otherwise sit behind a spinner for many
    /// seconds with nothing to read.
    ///
    /// Cancelling resolves with whatever text arrived, flagged `aborted` — a partial
    /// answer is still useful and stays in the thread.
    pub async fn stream_chat_with_ai(&self, messages: &Value, cancel: &CancellationToken, mut on_delta: impl FnMut(&str)) -> Result<Value, String> {
        let request = match self.build_chat_request(messages, true)? {
            Ok(r) => r,
            Err(result) => return Ok(result),
        };

        let send = self
            .ai_request(&request.url, &request.api_key, true, "text/event-stream")
            .header("Content-Type", "application/json")
            .body(jsjson::stringify(&request.body))
            .send();
        let response = tokio::select! {
            biased;
            _ = cancel.cancelled() => return Ok(aborted_reply("")),
            r = send => r.map_err(|e| fetch_error(&e, &request.url))?,
        };

        let status = response.status();
        if !status.is_success() {
            let text = tokio::select! {
                biased;
                _ = cancel.cancelled() => return Err(ABORTED.into()),
                t = response.text() => t.map_err(|_| BODY_TERMINATED.to_string())?,
            };
            return Ok(error_reply(&describe_http_error(status.as_u16(), &text)));
        }

        // If the server ignored `stream` and answered with a normal JSON completion, don't try
        // to parse it as SSE — fall back to reading it whole.
        let content_type = response.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("").to_string();
        if !content_type.contains("text/event-stream") {
            let text = tokio::select! {
                biased;
                _ = cancel.cancelled() => return Err(ABORTED.into()),
                t = response.text() => t.map_err(|_| BODY_TERMINATED.to_string())?,
            };
            let data = parse_json_body(&text)?;
            let whole = message_content(first_choice(&data));
            if !whole.is_empty() {
                on_delta(&whole);
            }
            return Ok(if js::trim(&whole).is_empty() { error_reply("The model returned an empty response.") } else { reply(&whole) });
        }

        let mut body = response.bytes_stream();
        let mut decoder = Utf8StreamDecoder::default();
        let mut buffer = String::new();
        let mut full = String::new();
        let mut finish = String::new();

        loop {
            let next = tokio::select! {
                biased;
                _ = cancel.cancelled() => return Ok(aborted_reply(&full)),
                n = body.next() => n,
            };
            let chunk = match next {
                None => break,
                Some(Ok(c)) => c,
                Some(Err(_)) => {
                    if cancel.is_cancelled() {
                        return Ok(aborted_reply(&full));
                    }
                    return Err(BODY_TERMINATED.into());
                }
            };
            buffer.push_str(&decoder.decode(&chunk));

            // SSE frames are separated by a blank line; keep the trailing partial in the buffer.
            let mut frames: Vec<String> = buffer.split("\n\n").map(str::to_string).collect();
            buffer = frames.pop().unwrap_or_default();

            for frame in frames {
                for line in frame.split('\n') {
                    let Some(rest) = line.strip_prefix("data:") else { continue };
                    let payload = js::trim(rest);
                    if payload.is_empty() || payload == "[DONE]" {
                        continue;
                    }
                    // A malformed frame shouldn't kill the stream; skip it and keep reading.
                    let Ok(parsed) = serde_json::from_str::<Value>(payload) else { continue };
                    let choice = first_choice(&parsed);
                    if let Some(r) = finish_reason(choice) {
                        finish = r;
                    }
                    if let Some(delta) = choice.and_then(|c| c.get("delta")).and_then(|d| d.get("content")).and_then(Value::as_str) {
                        if !delta.is_empty() {
                            full.push_str(delta);
                            on_delta(delta);
                        }
                    }
                }
            }
        }

        if cancel.is_cancelled() {
            return Ok(aborted_reply(&full));
        }

        if js::trim(&full).is_empty() {
            let reason = if finish.is_empty() { String::new() } else { format!(" (finish_reason: {finish})") };
            return Ok(error_reply(&format!("The model returned an empty response{reason}.")));
        }

        Ok(reply(&full))
    }

    /// Whether Open WebUI is configured (URL and API key set). No secrets.
    pub fn open_webui_config_status(&self) -> Value {
        let settings = self.settings().get();
        let configured = !setting(&settings, "openWebUiApiUrl").is_empty() && !setting(&settings, "openWebUiApiKey").is_empty();
        json!({ "configured": configured })
    }

    /// Fetch the model list from Open WebUI `GET {url}/models`. Model ids only; safe for the window.
    pub async fn fetch_open_webui_models(&self) -> Value {
        let settings = self.settings().get();
        let base = base_url(&settings);
        let api_key = setting(&settings, "openWebUiApiKey");

        if base.is_empty() || api_key.is_empty() {
            return json!({ "error": "Add API URL and API key, then try again." });
        }

        let url = format!("{base}/models");
        let result: Result<Value, String> = async {
            let response = self.ai_request(&url, &api_key, false, "application/json").send().await.map_err(|e| fetch_error(&e, &url))?;
            let status = response.status();
            let text = response.text().await.map_err(|_| BODY_TERMINATED.to_string())?;
            if !status.is_success() {
                return Ok(json!({ "error": describe_http_error(status.as_u16(), &text) }));
            }
            let data = parse_json_body(&text)?;
            let items = match &data {
                Value::Array(a) => a.as_slice(),
                Value::Object(o) => o.get("data").and_then(Value::as_array).map(Vec::as_slice).unwrap_or(&[]),
                _ => &[],
            };
            let mut ids: Vec<String> = Vec::new();
            for item in items {
                if let Some(id) = item.get("id").and_then(Value::as_str) {
                    // `[...new Set(ids)]`: first occurrence wins.
                    if !ids.iter().any(|x| x == id) {
                        ids.push(id.to_string());
                    }
                }
            }
            ids.sort_by(|a, b| locale_compare(a, b));
            Ok(json!({ "models": ids }))
        }
        .await;
        result.unwrap_or_else(|message| json!({ "error": message }))
    }
}

/// `TextDecoder.decode(chunk, { stream: true })`: UTF-8 with replacement characters, holding back
/// an incomplete sequence at the end of a chunk until the next one arrives.
#[derive(Default)]
struct Utf8StreamDecoder {
    pending: Vec<u8>,
}

impl Utf8StreamDecoder {
    fn decode(&mut self, chunk: &[u8]) -> String {
        self.pending.extend_from_slice(chunk);
        let mut out = String::new();
        let mut rest: &[u8] = &self.pending;
        loop {
            match std::str::from_utf8(rest) {
                Ok(s) => {
                    out.push_str(s);
                    rest = &[];
                    break;
                }
                Err(e) => {
                    let (good, bad) = rest.split_at(e.valid_up_to());
                    out.push_str(std::str::from_utf8(good).unwrap_or(""));
                    match e.error_len() {
                        Some(n) => {
                            out.push('\u{FFFD}');
                            rest = &bad[n..];
                        }
                        None => {
                            rest = bad;
                            break;
                        }
                    }
                }
            }
        }
        self.pending = rest.to_vec();
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decoder_holds_back_split_characters() {
        let mut d = Utf8StreamDecoder::default();
        let bytes = "é—".as_bytes();
        assert_eq!(d.decode(&bytes[..1]), "");
        assert_eq!(d.decode(&bytes[1..3]), "é");
        assert_eq!(d.decode(&bytes[3..]), "—");
        assert_eq!(d.decode(&[0xff, b'a']), "\u{FFFD}a");
    }

    #[test]
    fn http_errors() {
        assert!(describe_http_error(403, "  <!DOCTYPE html><p>x").contains("HTML error page"));
        assert!(describe_http_error(403, "<HTML>").contains("HTML error page"));
        assert_eq!(describe_http_error(500, " {\"detail\":\"model crashed\"} "), "API error (500): {\"detail\":\"model crashed\"}");
        assert_eq!(describe_http_error(500, &"x".repeat(300)).len(), "API error (500): ".len() + 200);
    }
}

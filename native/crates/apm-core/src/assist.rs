//! AI assistant (Open WebUI) and AWS CLI docs. Port of `aiService.ts`, `awsCliDocsService.ts`
//! and `browserFetchService.ts`, plus their handler wrappers in `ipcHandlers.ts`.

mod ai;
mod cli_docs;
mod collate;

pub use cli_docs::{normalize_slug, parse_commands, parse_service_list, AWS_CLI_BASE_REFERENCE, AWS_CLI_INDEX_URL};
pub use collate::locale_compare;

use std::collections::HashMap;
use std::sync::Mutex;

use serde_json::{json, Value};
use tokio_util::sync::CancellationToken;

use crate::core::{Core, DispatchError, DispatchResult};
use crate::json as jsjson;

/// Per-core state of this area: in-flight AI streams by request id, so the Stop button can
/// abort them (`activeAiStreams` in ipcHandlers.ts).
#[derive(Default)]
pub struct AssistState {
    streams: Mutex<HashMap<String, CancellationToken>>,
}

/// Map key for a request id (1.x used the raw value as a `Map` key).
fn stream_key(id: &Value) -> String {
    match id {
        Value::String(s) => s.clone(),
        other => jsjson::stringify(other),
    }
}

/// JavaScript truthiness, for `if (!requestId)`.
fn truthy(v: &Value) -> bool {
    match v {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0 && !f.is_nan()),
        Value::String(s) => !s.is_empty(),
        _ => true,
    }
}

fn arg(args: &[Value], i: usize) -> &Value {
    args.get(i).unwrap_or(&Value::Null)
}

/// `payload?.messages ?? []`.
fn messages(payload: &Value) -> Value {
    payload.get("messages").filter(|m| !m.is_null()).cloned().unwrap_or_else(|| json!([]))
}

impl Core {
    /// Streaming chat. Resolves with the complete reply when the stream ends, while tokens
    /// are pushed to the window as they arrive on `ai:chat-chunk`. Keeping the final text
    /// on the result means the window doesn't have to reassemble it from chunks.
    pub async fn ai_chat_stream(&self, payload: &Value) -> Value {
        let request_id = payload.get("requestId").cloned().unwrap_or(Value::Null);
        if !truthy(&request_id) {
            return ai::error_reply("Missing request id.");
        }
        let key = stream_key(&request_id);
        let token = CancellationToken::new();
        self.assist.streams.lock().unwrap().insert(key.clone(), token.clone());

        let result = self
            .stream_chat_with_ai(&messages(payload), &token, |delta| {
                self.host.emit("ai:chat-chunk", vec![json!({ "requestId": request_id, "delta": delta })]);
            })
            .await;
        self.assist.streams.lock().unwrap().remove(&key);
        result.unwrap_or_else(|message| ai::error_reply(&format!("AI request failed: {message}")))
    }

    /// Stop an in-flight stream; unknown ids are fine.
    pub fn ai_chat_abort(&self, request_id: &Value) -> Value {
        if let Some(token) = self.assist.streams.lock().unwrap().get(&stream_key(request_id)) {
            token.cancel();
        }
        json!({ "ok": true })
    }

    /// The channels this area handles; `None` = not this area's channel.
    pub(crate) async fn dispatch_assist(&self, channel: &str, args: &[Value]) -> Option<DispatchResult> {
        let failed = DispatchError::Failed;
        let r = match channel {
            // AI: Open WebUI chat via REST (the API key stays in the core)
            "ai:chat" => self.chat_with_ai(&messages(arg(args, 0))).await.map_err(failed),
            "ai:chat-stream" => Ok(self.ai_chat_stream(arg(args, 0)).await),
            "ai:chat-abort" => {
                // The preload method takes the id and sends `{ requestId }` on the channel;
                // accept either shape.
                let a = arg(args, 0);
                let id = if a.is_object() { a.get("requestId").cloned().unwrap_or(Value::Null) } else { a.clone() };
                Ok(self.ai_chat_abort(&id))
            }
            "ai:getConfigStatus" => Ok(self.open_webui_config_status()),
            "ai:getModels" => Ok(self.fetch_open_webui_models().await),

            // AWS CLI docs (scraped service list + per-service commands, cached on disk).
            // The two "getCached" channels run the window-facing composed methods
            // (`getAwsCliServiceList` / `getAwsCliCommandsForService` in the preload: cache,
            // else fetch + parse + cache), which is what the IPC contract maps them to; in 2.0
            // the fetch happens in the core, so the window never sees the HTML.
            "awsCli:getCachedServiceList" => self.aws_cli_service_list().await.map_err(failed),
            "awsCli:getCachedCommands" => self.aws_cli_commands_for_service(arg(args, 0).as_str().unwrap_or("")).await.map_err(failed),
            "awsCli:parseAndCacheServiceList" => self.parse_and_cache_service_list(arg(args, 0).as_str().unwrap_or("")).map_err(failed),
            "awsCli:parseAndCacheCommands" => {
                self.parse_and_cache_commands(arg(args, 0).as_str().unwrap_or(""), arg(args, 1).as_str().unwrap_or("")).map_err(failed)
            }
            "awsCli:fetchWithBrowser" => self.fetch_docs_html(arg(args, 0).as_str().unwrap_or("")).await.map(Value::String).map_err(failed),
            _ => return None,
        };
        Some(r)
    }
}

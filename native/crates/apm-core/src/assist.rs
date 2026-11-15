//! AI assistant (Open WebUI) and AWS CLI docs. Port of `aiService.ts`, `awsCliDocsService.ts`
//! and `browserFetchService.ts`.

use serde_json::Value;

use crate::core::{Core, DispatchResult};

impl Core {
    /// The channels this area handles; `None` = not this area's channel.
    pub(crate) async fn dispatch_assist(&self, channel: &str, args: &[Value]) -> Option<DispatchResult> {
        let _ = (channel, args);
        None
    }
}

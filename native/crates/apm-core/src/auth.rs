//! SAML sign-in, STS, role fetching and profile refresh. Port of `awsAuthService.ts`.

use serde_json::Value;

use crate::core::{Core, DispatchResult};

impl Core {
    /// The channels this area handles; `None` = not this area's channel.
    pub(crate) async fn dispatch_auth(&self, channel: &str, args: &[Value]) -> Option<DispatchResult> {
        let _ = (channel, args);
        None
    }
}

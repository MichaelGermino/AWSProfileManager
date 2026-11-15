//! Backup and restore, org config file, release notes, tour, console sign-in, browsers, app
//! icons, background video, updater. Ports of the matching 1.x services.

use serde_json::Value;

use crate::core::{Core, DispatchResult};

impl Core {
    /// The channels this area handles; `None` = not this area's channel.
    pub(crate) async fn dispatch_extras(&self, channel: &str, args: &[Value]) -> Option<DispatchResult> {
        let _ = (channel, args);
        None
    }
}

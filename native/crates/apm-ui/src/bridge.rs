//! The UI's way to the core. Views never touch files, the network or the keychain: they call IPC
//! channels on `apm_core::Core` (the same contract the 1.x window used) and react to its events.
//!
//! The core runs on a tokio runtime owned by the app; GPUI has its own executor. A call is spawned
//! on the runtime and its result comes back over a oneshot the GPUI side awaits.

use std::sync::Arc;

use apm_core::core::{Core, DispatchError};
use futures::channel::oneshot;
use serde_json::Value;

/// One event from the core (`Host::emit`): channel plus positional payload.
#[derive(Debug, Clone)]
pub struct CoreEvent {
    pub channel: String,
    pub payload: Vec<Value>,
}

#[derive(Clone)]
pub struct Bridge {
    core: Arc<Core>,
    runtime: tokio::runtime::Handle,
}

impl Bridge {
    pub fn new(core: Arc<Core>, runtime: tokio::runtime::Handle) -> Self {
        Bridge { core, runtime }
    }

    pub fn core(&self) -> &Arc<Core> {
        &self.core
    }

    /// Run one channel. The future resolves on any executor (GPUI's included).
    pub fn call(&self, channel: &str, args: Vec<Value>) -> impl std::future::Future<Output = Result<Value, String>> + 'static {
        let (tx, rx) = oneshot::channel();
        let core = self.core.clone();
        let channel = channel.to_string();
        self.runtime.spawn(async move {
            let result = core.dispatch(&channel, &args).await.map_err(|e| match e {
                DispatchError::Failed(m) => m,
                DispatchError::Unsupported(c) => format!("not available: {c}"),
            });
            let _ = tx.send(result);
        });
        async move { rx.await.unwrap_or_else(|_| Err("the app is shutting down".into())) }
    }
}

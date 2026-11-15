//! IAM Identity Center: OIDC sign-in (PKCE, loopback redirect), portal APIs, the SSO session store.
//! Port of `identityCenterService.ts` and `ssoTokenStore.ts`.

use serde_json::Value;

use crate::core::{Core, DispatchResult};
use crate::network_classifier::ErrorInfo;
use crate::sso_org::SsoOrg;

/// Role credentials from the portal's GetRoleCredentials.
#[derive(Debug, Clone)]
pub struct SsoRoleCredentials {
    pub access_key_id: String,
    pub secret_access_key: String,
    pub session_token: String,
    /// Epoch ms.
    pub expiration_ms: i64,
}

impl Core {
    /// The channels this area handles; `None` = not this area's channel.
    pub(crate) async fn dispatch_sso(&self, channel: &str, args: &[Value]) -> Option<DispatchResult> {
        let _ = (channel, args);
        None
    }

    /// A usable access token for `org` (1.x `getAccessToken`). `Ok(None)`: a human must sign in
    /// (never an error). `interactive: false` (the scheduler) never opens a browser.
    pub async fn sso_access_token(&self, org: &SsoOrg, interactive: bool) -> Result<Option<String>, ErrorInfo> {
        let _ = (org, interactive);
        Err(ErrorInfo { message: Some("Identity Center is not ported yet".into()), ..Default::default() })
    }

    /// 1.x `getRoleCredentials`.
    pub async fn sso_role_credentials(
        &self,
        org: &SsoOrg,
        access_token: &str,
        account_id: &str,
        role_name: &str,
    ) -> Result<SsoRoleCredentials, ErrorInfo> {
        let _ = (org, access_token, account_id, role_name);
        Err(ErrorInfo { message: Some("Identity Center is not ported yet".into()), ..Default::default() })
    }
}

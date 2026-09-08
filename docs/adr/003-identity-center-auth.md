# ADR 003: IAM Identity Center profiles alongside SAML

## Context

- Newer AWS accounts are reached through **IAM Identity Center federated to Entra ID**, not ADFS. The existing SAML flow (form post → SAML assertion → `AssumeRoleWithSAML`) does not apply to them.
- We cannot register an application in the Entra tenant — no admin rights.
- MFA is enforced on the Entra side, so the stored IdP password cannot be replayed the way it is for ADFS.
- The organization issues **two AD identities per person**: a normal account and a privileged (SA) account. Only the SA account has AWS access, and the default browser is habitually signed into the normal account.
- Downstream consumers (AWS CLI, locally-developed .NET apps) read `~/.aws/credentials`. Whatever we build must land there in the same format.

## Decision

**Add `Profile.authType` (`'saml' | 'identityCenter'`, absent ⇒ `'saml'`) and dispatch inside `refreshProfile`.** That single seam keeps `refreshScheduler`, `ipcHandlers`, `dashboardService`, the tray, and `configBackup` unchanged — they only read `credentialProfileName`, `expiration` and `roleDisplayText`, all of which Identity Center populates identically.

**Authenticate with Identity Center's own OIDC service** (`oidc.<region>.amazonaws.com`) using dynamic client registration plus the authorization-code + PKCE grant. `RegisterClient` is unauthenticated, so the app registers itself *with AWS* and the Entra tenant never needs to know it exists. Credentials come from `GetRoleCredentials` on `portal.sso.<region>.amazonaws.com` and are written by the existing `writeCredentialsForProfile`.

**Call those REST APIs directly over `node:https` rather than adding `@aws-sdk/client-sso{,-oidc}`.** The surface is six calls; adding SDK packages would disturb the `overrides` block that keeps `npm audit` at zero (see CLAUDE.md), for no functional gain. The enterprise TLS agent is attached explicitly, exactly as `buildStsConfig` does for STS.

**Key the SSO session on the org (start URL + region), not on profileId.** One session serves every profile in the org — a different cardinality from IdP credentials, which are per-profile.

**Store SSO sessions in a `safeStorage`-encrypted file, not Keytar.** Windows Credential Manager caps a credential blob at 2560 bytes (`CRED_MAX_CREDENTIAL_BLOB_SIZE`); SSO access and refresh tokens run 1–2 KB each, so `keytar.setPassword` throws. This was found in testing, where the symptom was AWS re-showing its consent screen on every single sign-in — the session never persisted, so each call registered a fresh OIDC client. Sessions now live in `sso-sessions.json` in app data, each value encrypted with Electron `safeStorage` (DPAPI, bound to the OS user) and wrapped in the `v1:` AES-256-GCM layer first when a master password is set. No plaintext fallback. Sessions are additionally cached in memory for the process lifetime, so a persistence failure costs a re-login after restart rather than on every call.

**Sign in through an embedded `BrowserWindow` on a per-org persistent partition** (`persist:sso-<hash>`) by default. Isolated from the default browser's session the way a private window is, but — unlike a private window — it keeps the SA session across app restarts. Measured: a warm session plus a cached client registration completes sign-in in 0.8s with no interaction. `settings.ssoBrowserMode = 'external'` falls back to `shell.openExternal` if Conditional Access rejects embedded webviews.

**Treat "needs a browser sign-in" as a distinct outcome, not a failure.** `refreshProfile` returns `{ ssoLoginRequired: true, … }`; it does not increment `refreshFailureCounters` and cannot trip the 2-strike auto-refresh pause. **The scheduler never opens a browser** — it attempts only the silent refresh-token path and emits `auth:ssoLoginRequired` if that fails. Prompts are coalesced per org, and one in-flight login promise per org means concurrent callers share a single window.

## Alternatives considered

- **Adding the AWS SDK SSO clients**: rejected — dependency risk against the `overrides` block outweighs the benefit for six REST calls.
- **`shell.openExternal` as the default**: rejected — it opens the browser holding the *normal* AD account and would silently authenticate the wrong identity.
- **Device authorization grant as primary**: rejected as the default — it always requires an "Allow" click. Retained as a fallback shape.
- **Scripting the Entra login in a hidden window using the stored password**: rejected — defeats MFA, breaks whenever Microsoft reskins the login page, and Conditional Access increasingly blocks embedded webviews for credential entry.
- **Writing `[sso-session]` blocks to `~/.aws/config`** so `aws sso login` works outside the app: rejected as out of scope — the app already writes `~/.aws/credentials`, which is what the CLI and the .NET apps consume.
- **Re-encrypting SSO tokens when a master password is created**: rejected — the tokens are deleted instead, costing one browser sign-in and avoiding a migration path.

## Consequences

- Identity Center profiles have a different lifecycle from SAML ones: silent almost always, real browser occasionally. The UI states this rather than implying they are identical.
- The stored IdP username/password is unused by these profiles and the "Use default credentials" checkbox is hidden for them.
- Consent ("Allow AWSProfileManager to access your data") appears once per client registration — roughly quarterly, since the client secret lives ~90 days. The registered client name is user-visible.
- `credentialStorage`'s master-password functions iterate `getProfiles()` and key Keytar by `p.id`; SSO sessions use org-derived names, so those loops were extended explicitly. Any future secret keyed differently must do the same or it will be silently skipped when a master password is enabled or reset.
- The **SSO session duration** — the admin-configured value that bounds refresh-token life and therefore sets the real re-auth cadence — is not exposed by any API and remains unmeasured. The access token's 1h `expiresIn` is *not* it.

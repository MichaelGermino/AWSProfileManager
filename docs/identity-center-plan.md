# Plan: IAM Identity Center (Entra) profile support

Status: **implemented** (phases 0–6). See `adr/003-identity-center-auth.md` for the decisions as
built. This document is kept for the Phase 0 measurements and the reasoning behind them.

Deviations from the plan below, all deliberate:

- **No new npm dependencies.** `@aws-sdk/client-sso{,-oidc}` were dropped in favor of direct
  `node:https` calls, so the `overrides` block is untouched and `npm audit` is unaffected.
  Consequently the planned `awsClientConfig.ts` extraction was unnecessary and `buildStsConfig`
  stays where it is.
- **`ssoOrg.ts` was split.** The renderer imports it, so `node:crypto` (the hashed Keytar/partition
  name) had to move to `src/main/services/ssoOrgKey.ts`.
- **Embedded `BrowserWindow` is the default sign-in surface**, not `shell.openExternal` — see the
  browser-strategy section below.

## Goal

Support AWS accounts reached through **IAM Identity Center federated to Entra ID**, alongside the
existing ADFS/SAML profiles, with no visible difference downstream: credentials land in
`~/.aws/credentials` under `credentialProfileName` exactly as they do today, so the AWS CLI and
locally-developed .NET apps are unaffected.

Reference org (do **not** hardcode — these are per-profile config fields):

- Start URL `https://d-9267c700e2.awsapps.com/start`
- SSO region `us-west-2`

Both values are displayed to the user in the access portal under
*any account → Access keys → "AWS IAM Identity Center credentials"*. The Identity Center instance
id (`ssoins-…`) shown on that page is for admin-side APIs and is **not** needed.

## Why this works without Entra admin rights

IAM Identity Center runs its own OIDC service (`oidc.<region>.amazonaws.com`) supporting
**dynamic client registration**: `RegisterClient` is an unauthenticated call any client may make.
The app registers itself *with AWS*, never with the Entra tenant. Entra only ever sees the AWS
access portal application that is already registered there.

Entra Conditional Access policies that block the *device code flow* do not apply: the user's
Entra authentication happens as an ordinary interactive browser redirect from the AWS portal.

## Auth flow

**Primary: authorization code + PKCE** (what AWS CLI v2 uses when it can open a browser).

1. Bind a loopback listener on a fixed port (54321, scan upward if taken) →
   `redirectUri = http://127.0.0.1:<port>/oauth/callback`.
2. `RegisterClient({ clientName: 'AWSProfileManager', clientType: 'public',
   scopes: ['sso:account:access'], grantTypes: ['authorization_code', 'refresh_token'],
   redirectUris: [redirectUri], issuerUrl: startUrl })` → clientId/clientSecret, ~90 day life.
   Cached; re-registered on expiry or if the port changed.
3. `shell.openExternal()` the `/authorize` URL with `code_challenge_method=S256` and a random
   `state`.
4. User's browser hits the AWS portal → redirects to Entra. On the corporate network with a live
   M365 session this completes with **no prompts and no clicks**; off-network it prompts for MFA.
5. Loopback listener receives `?code=&state=`, verifies `state`, responds with a small
   "you can close this window" page.
6. `CreateToken({ grantType: 'authorization_code', code, codeVerifier, redirectUri, … })` →
   **access token** (life = Identity Center portal session duration) + **refresh token**.

**Consent screen.** Contrary to an early assumption in this plan, the PKCE flow *does* show an
"Allow &lt;clientName&gt; to access your data?" page — observed in the Phase 0 spike. The open
question is whether it appears on every sign-in or only on first use of a given registered client;
since the app caches its registration for the client's ~90 day life, the latter would make it a
quarterly event. Being measured. Either way the client name is user-visible, so it must read
`AWSProfileManager`.

**Fallback: device authorization grant** if the authorize endpoint rejects the code grant.
`StartDeviceAuthorization` → show `userCode` + open `verificationUriComplete` → poll `CreateToken`
handling `AuthorizationPendingException` / `SlowDownException`.

**Steady state is silent.** Renewal via `CreateToken({ grantType: 'refresh_token' })` is a plain
HTTPS call — no browser, no window, no user — and so is `GetRoleCredentials`. A browser round-trip
happens only when the refresh token itself dies.

**Per-profile refresh**, once a valid access token exists:

```
GetRoleCredentials({ accessToken, accountId: profile.ssoAccountId, roleName: profile.ssoRoleName })
  → { accessKeyId, secretAccessKey, sessionToken, expiration }
  → writeCredentialsForProfile(profile.credentialProfileName, …)   // unchanged code path
```

**Account discovery**: `ListAccounts` + `ListAccountRoles` with the same bearer token — this is the
API behind the account list the portal shows.

## Measured facts (Phase 0 spike, 2026-09-08)

Against `d-9267c700e2` / `us-west-2`, SA account, Edge InPrivate:

| Measurement | Value |
| --- | --- |
| PKCE authorization code grant | **Works.** `scopes` (plural) is the correct authorize param |
| Consent screen | Shown once on **first use of a registered client**; not shown again when the `clientId` was reused |
| Access token lifetime | 3600s — fixed by Identity Center, renewed silently |
| Silent `refresh_token` renewal | **Works.** Refresh token is *not* rotated |
| Permission set session duration | 60 minutes (the default) |
| SSO session duration | **Still unknown** — see below |
| Cold sign-in (private window) | 23.6s including MFA |
| Warm sign-in (session + cached client) | **0.8s, zero interaction** |
| Accounts / roles visible | 7 accounts, 9 account-role pairs (`RegionalAdmin`, `S3_FullAccess`) |

Three consequences:

1. **The 0.8s warm run validates the persistent-partition design.** Run B reused the still-open
   InPrivate session plus the cached client registration and completed with no sign-in, no consent
   and no clicks. A `BrowserWindow` with a `persist:` partition reproduces exactly that state, and
   unlike an InPrivate window it survives app restarts.
2. **Consent is a once-per-registration event**, so it costs the user one click roughly quarterly,
   not one per sign-in. The registered `clientName` is user-visible on that screen.
3. **`ListAccounts` returns accounts in non-deterministic order** — the two runs differed. The
   import wizard must sort (by account name) or the checkbox list reshuffles between visits.

### The re-auth cadence is still unmeasured

The access token's 3600s `expiresIn` is **not** the re-auth cadence — Identity Center fixes access
tokens at 1h and the refresh token renews them silently. The number that actually matters is the
admin-configured **SSO session duration** (1h–90d, default 8h), which bounds how long the refresh
token keeps working. No API reports it; the only way to learn it is to refresh until rejection.

`sso-spike.mjs --soak` does exactly that, logging to `sso-soak-log.txt`. Until it reports, treat the
re-auth cadence as unknown-but-at-least-1h. It does not block Phases 1–4; it sets the tone of the
Phase 5 re-auth UX.

## Data model

`src/shared/types.ts`:

```ts
export type ProfileAuthType = 'saml' | 'identityCenter';

export interface Profile {
  // …existing fields unchanged…
  /** undefined means 'saml' — existing profiles migrate for free. */
  authType?: ProfileAuthType;

  // Identity Center only:
  ssoStartUrl?: string;   // normalized, no '#/' fragment
  ssoRegion?: string;     // e.g. 'us-west-2'
  ssoAccountId?: string;  // 12-digit
  ssoRoleName?: string;   // permission set name
}
```

`idpEntryUrl` / `roleArn` / `principalArn` become SAML-only in practice. `roleDisplayText` is still
populated (`"123456789012 / MyRole"`), so the dashboard, tray, terminal picker, expiration countdown
and delete-cleanup need **zero changes** — they only read `credentialProfileName`, `expiration`
and `roleDisplayText`.

**Start URL + region are denormalized onto each profile** rather than living in a new
`ssoSessions` store. This matches how `idpEntryUrl` already works, keeps `configBackup` working
untouched, and adds no new persistence file. Profiles sharing an org converge automatically because
the token store keys on `sha256(startUrl + '|' + region)`.

## New main-process code

| File | Responsibility |
| --- | --- |
| `services/identityCenterService.ts` | PKCE/device login, `ListAccounts`/`ListAccountRoles`, `GetRoleCredentials`, per-org in-flight login coalescing |
| `services/ssoTokenStore.ts` | Keytar-backed registration + token cache, keyed `sso:<hash(startUrl\|region)>` |
| `services/awsClientConfig.ts` | `buildAwsClientConfig(region)` — extracted from `buildStsConfig()` |

Dependencies: `@aws-sdk/client-sso-oidc` and `@aws-sdk/client-sso`, matching the existing
`@aws-sdk/client-sts` version line (`^3.700.0`).

No new top-level `dist/` subdirectory is created (these live under `src/main/services/`), so
`scripts/write-cjs-markers.mjs` needs no change.

## Wiring into the existing seams

**`refreshProfile` becomes a dispatcher** (`awsAuthService.ts:528`). This is the single seam —
`refreshScheduler`, `ipcHandlers`, `dashboardService` and the tray all keep calling it unchanged:

```ts
const profile = getProfileById(profileId);
if (!profile) { /* unchanged */ }
if ((profile.authType ?? 'saml') === 'identityCenter') {
  return refreshIdentityCenterProfile(profile);
}
// existing SAML path from here down
```

⚠️ The `if (!profile.idpEntryUrl?.trim())` guard at `awsAuthService.ts:542` currently rejects any
profile without an IdP URL. It must move **below** the dispatch, into the SAML branch.

**New `RefreshResult` variant** `{ ssoLoginRequired: true; profileId: string }`, added to **both**
union declarations — `src/shared/types.ts:89` and `awsAuthService.ts:522`. (Those two are already
duplicated and divergent; the shared one is missing the `{ roles, profileId }` variant. Worth
reconciling while in there.)

**Credential storage gotcha (security-relevant).** `getMasterPasswordStatus`,
`createMasterPassword`, `unlockWithMasterPassword` and `forgetAllCredentialsAndResetMasterPassword`
in `credentialStorage.ts` all iterate `getProfiles()` and use `p.id` as the Keytar account name.
SSO token entries use a different account key and would be **silently skipped** — left in plaintext
when a master password is enabled, and left behind on reset.

Resolution: encrypt SSO tokens at rest with the same `v1:` AES-256-GCM scheme, and on master
password *create / reset*, **delete** the SSO token entries rather than re-encrypting them. Costs
one browser sign-in and avoids a re-encryption migration path entirely.

**Scheduler** (`refreshScheduler.ts:73`):

- `ssoLoginRequired` is **not** a failure — it must not increment `refreshFailureCounters` or trip
  the 2-strike auto-refresh pause. It is a waiting-on-human state, same as the existing
  `CredentialsRequiredResult`.
- The scheduler **never opens a browser**. It attempts the silent refresh-token path only. If that
  fails it emits `auth:ssoLoginRequired` and stops; a browser opens only from an explicit user
  action (clicking that prompt, or hitting Refresh).
- Emissions are **coalesced per org**, not per profile — eight profiles in one org must produce one
  prompt, and one successful sign-in must satisfy all eight. `identityCenterService` holds an
  in-flight login promise per org key that concurrent callers await.

**IPC** — three files must stay in sync (`ipcHandlers.ts`, `preload/index.ts`, renderer call sites):

| Channel | Direction | Purpose |
| --- | --- | --- |
| `sso:signIn` | invoke | Start interactive login for an org; resolves on success/cancel |
| `sso:getSessionStatus` | invoke | `{ signedIn, expiresAt }` for a start URL, for badges |
| `sso:listAccounts` | invoke | Accounts + roles for the import wizard |
| `sso:signOut` | invoke | Drop cached token/registration for an org |
| `sso:detectRegion` | invoke | Probe candidate regions (fallback for users whose portal doesn't show it) |
| `auth:ssoLoginRequired` | event → renderer | Org session dead; prompt user to sign in |
| `sso:loginProgress` | event → renderer | Waiting / device user-code / completed |

## Renderer work

`src/renderer/pages/Profiles.tsx` (1,197 lines) is where nearly all of it lands:

- `emptyProfile()` (line 139) — add `authType: 'saml'` default.
- `canSave` guard (line 304) — currently requires `form.idpEntryUrl`; must branch on auth type.
- Profile form (~line 829) — an auth-type toggle at the top; SAML shows today's fields, Identity
  Center shows Start URL + Region + "Sign in & choose account".
- The Identity Center analogue of "Fetch roles" is an account/role picker fed by `sso:listAccounts`,
  reusing the existing `RoleModal` shape.
- New `SsoLoginModal` — "waiting for browser sign-in…" with cancel, plus the device user-code when
  the fallback grant is in play.
- Small badge on SSO profiles in the list, and a "Sign in" action when the org session is dead.

`App.tsx` registers the `auth:ssoLoginRequired` listener alongside the existing
`onCredentialsRequired`.

## Bulk import wizard

The seamlessness requirement. Enter start URL + region → sign in once → checkbox tree of every
account/role the user has → create N profiles in one action, auto-filling `name`,
`credentialProfileName`, `roleDisplayText` (honoring `settings.accountDisplayNames`) and
`refreshIntervalMinutes`. This is strictly better onboarding than the per-profile SAML flow and is
the main reason to do the discovery APIs properly.

## Phases

| # | Scope | Exit criteria |
| --- | --- | --- |
| 0 | Throwaway spike script — no app changes | PKCE confirmed click-free on the corporate network; prints access token `expiresIn` (= portal session duration) and `GetRoleCredentials` expiry (= permission set duration) |
| 1 | Types, `ssoTokenStore`, `identityCenterService`, TLS config extraction, `refreshProfile` dispatch | A hand-written `profiles.json` entry refreshes into `~/.aws/credentials` |
| 2 | IPC + preload surface | Channels callable from devtools |
| 3 | Profile form auth-type toggle, sign-in modal, account picker | A profile can be created and refreshed entirely from the UI |
| 4 | Bulk import wizard | All accounts imported in one pass |
| 5 | Scheduler integration: silent refresh, per-org coalescing, failure-counter exemption, notifications | Eight profiles, one prompt; unattended overnight run stays refreshed |
| 6 | `docs/adr/003-identity-center-auth.md`; update `architecture.md`, `domain-model.md`, `security-model.md`, `ai-constraints.md`, `CLAUDE.md` | Docs match behavior |

Phases 1–3 deliver a working single profile end-to-end. Phase 4 delivers the seamlessness. Phase 5
makes it production-worthy.

## Open items

- **Phase 0 resolves the API details.** The device grant shape is well documented; the exact
  `/authorize` query parameters for the PKCE grant (notably `scopes` vs `scope`) should be confirmed
  against the live endpoint rather than assumed.
- **Both session durations are unknown** and the portal does not display them. Phase 0 reads them
  off the wire. They set the default `refreshIntervalMinutes` for SSO profiles and determine how
  often a browser trip is needed. If the portal session turns out to be the 8h default, ask the
  Identity Center admin whether policy allows raising it — that converts "sign in every morning"
  into "sign in Monday".
- **Adding two npm dependencies touches the load-bearing `overrides` block.** Verify with a real
  `npm run dist`, not just `npm run build` — a broken installer is the failure mode.
### Browser strategy — `shell.openExternal` is wrong for this org

The organization issues **two AD identities per person**: a normal account and an **SA account**,
and only the SA account has AWS access. The normal browser session is always signed into the normal
account, so staff routinely use a private window to reach AWS at all.

Consequences:

1. **`shell.openExternal` to the default browser silently authenticates as the wrong identity.**
   It cannot be the default.
2. **The "warm session makes it click-free" argument disappears.** That was the main reason to
   prefer an external browser over an embedded one, and it does not apply here — a private window
   starts cold every time, so every interactive login is a full sign-in plus MFA.
3. **Refresh token lifetime therefore dominates the UX.** It is the only thing standing between the
   user and a full SA sign-in on every refresh.

Preferred approach, to be confirmed: an Electron `BrowserWindow` with a **dedicated persistent
partition** (`persist:sso-<hash(startUrl)>`). This is isolated from the normal AD session the way a
private window is, but unlike a private window it *keeps* the SA session across app restarts —
strictly better than incognito for this use case, and the app can auto-close it on callback and read
back the signed-in identity.

Risk: Entra Conditional Access may reject embedded webviews ("require approved client app" /
"require compliant device"). If it does, fall back to spawning the detected browser's private window
directly (`--inprivate` / `--incognito` / `-private-window`), passing the URL as a literal argv
entry.

⚠️ **Never launch a URL through `cmd /c start`** — cmd treats `&` as a command separator and
truncates the OAuth URL at its first query parameter, producing a misleading
`invalid_request: Client ID is required` from the authorize endpoint. Spawn the executable directly
with an args array, or use `rundll32 url.dll,FileProtocolHandler`. (Hit during the Phase 0 spike.)

Regardless of approach, **display the authenticated identity after sign-in** so a wrong-account
login is immediately obvious rather than silently producing credentials for the wrong principal.

## Explicitly out of scope

- **Writing `[sso-session]` / `[profile]` blocks to `~/.aws/config`.** Only useful for people
  bypassing the app; the app already writes `~/.aws/credentials`, which is what the CLI and the
  local .NET apps consume. Identity Center profiles land in that same file in that same format.
- **Replaying the stored Entra password.** There is no supported way to feed the Keytar password
  into the Entra flow; it would require an embedded webview scraping the Microsoft login form, which
  Conditional Access increasingly blocks and which breaks whenever Microsoft reskins the page. The
  stored SA credentials are simply not usable for these accounts — SSO profiles get a different
  lifecycle (silent almost always, real browser occasionally), and the UI should say so rather than
  pretend the two profile types are identical.

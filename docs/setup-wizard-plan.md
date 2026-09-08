# Plan: setup wizard + SAML bulk import

Status: proposed.

## Goals

1. **First-run setup.** A new user opens the app and is walked through master password → default
   credentials → profiles → optional integrations, able to skip any step.
2. **One component, two entry points.** The same wizard, scoped to a subset of steps, backs the
   "Import" action later. There is no second implementation to keep in sync.
3. **SAML bulk import.** Today Identity Center profiles can be imported en masse but SAML profiles
   are created one at a time. Close that gap.
4. **Both importers seed `accountDisplayNames`**, so the profile list shows friendly names.

## The dependency chain is the hard constraint

Step order is not a style choice — the data forces it:

```
master password ──► default credentials ──► SAML role list ──► SAML profiles
                                              (needs stored username+password)

Identity Center ──► browser sign-in ──► account list ──► SSO profiles
   (independent; needs no master password and no stored credentials)
```

- `setDefaultCredentials` **refuses** without a master password (returns `MASTER_PASSWORD_REQUIRED`),
  so the master-password step must precede it.
- `fetchRolesForIdp` requires `useDefaultCredentials: true` **and** stored credentials, otherwise it
  returns `credentialsRequired`. `fetchRolesWithCredentials(url, user, pass)` is the escape hatch
  when the user would rather not store anything — the wizard should use it for a "just this once"
  path so SAML import still works for someone who skipped the credentials step.
- The Identity Center branch has no such chain, so a user who only has SSO accounts can skip
  straight past master password and credentials.

Consequence: **skipping must be non-linear.** A wizard that forces a straight line will strand the
SSO-only user on a master-password prompt they do not need.

## Step inventory

| Step | Skippable | Notes |
| --- | --- | --- |
| Welcome | — | What the app does; "Skip setup" exits the whole wizard |
| Master password | yes | Required before credentials can be stored; explain that plainly |
| Default credentials | yes | IdP username/password; gated on the step above |
| Add accounts — choose type | — | SAML, Identity Center, both, or neither |
| SAML import | yes | IdP URL → fetch roles → checkbox list → create N profiles |
| Identity Center import | yes | Start URL + region → sign in → checkbox list → create N profiles |
| Terminal shell | yes | PowerShell vs bash + path; trivial, low value, consider dropping |
| AI assistant (Open WebUI) | yes | Niche. Must be clearly optional and last |
| Done | — | Summary of what was created, link into Settings |

## SAML bulk import — parity with the Identity Center importer

The two are structurally the same and should share the picker UI.

|  | Identity Center | SAML |
| --- | --- | --- |
| Discovery | `sso:listAccounts` → `SsoAccount[]` | `auth:fetchRoles` → `AwsRole[]` |
| Account id | `accountId` | parsed from `roleArn` (`arn:aws:iam::(\d+):role/(.+)`) |
| Role name | `roleName` | second capture group of the same regex |
| Friendly name | `accountName` | `AwsRole.accountName` (scraped from the IdP page) |
| Profile fields | `authType: 'identityCenter'`, `ssoStartUrl`, `ssoRegion`, `ssoAccountId`, `ssoRoleName` | `authType: 'saml'`, `idpEntryUrl`, `roleArn`, `principalArn` |

Everything else — name generation, `credentialProfileName` slugging with collision suffixes,
`roleDisplayText`, `autoRefresh`, `refreshIntervalMinutes` — is already written in
`handleConfirmImport` and should be lifted into a shared helper rather than copied.

`AwsRole.accountName` is populated only when the IdP page exposes it, so the SAML importer must
fall back to the bare account id for both the display name and the generated profile name.

## Seeding account display names

Both importers should merge `{ accountId: accountName }` into `settings.accountDisplayNames`,
fill-if-empty so hand-edited names survive. The SSO importer already does this; SAML needs it, and
the merge logic should move into the shared helper alongside profile creation.

Note this is now only a *convenience*: `dashboardService.resolveDisplayText` looks the name up at
read time, so seeding no longer has to happen before `roleDisplayText` is computed.

## First-run detection and re-entry

Add `settings.setupCompleted?: boolean`.

- Wizard opens automatically when `!setupCompleted` **and** `getProfiles().length === 0`. Both
  conditions, so an existing user who upgrades is never interrupted.
- "Skip setup" and "Done" both set `setupCompleted: true`.
- Re-entry points: the split button's menu on the Profiles page (replacing "Import from Identity
  Center…" with "Add accounts…"), and a "Run setup again" button in Settings → General.
- The import entry opens the wizard with `steps={['chooseType', 'samlImport', 'ssoImport', 'done']}`,
  skipping welcome/master-password/credentials entirely.

## Architecture

```
src/renderer/wizard/
  SetupWizard.tsx        // shell: step list, progress rail, transitions, skip/back/next
  useWizardSteps.ts      // which steps are active for this invocation
  steps/WelcomeStep.tsx
  steps/MasterPasswordStep.tsx
  steps/CredentialsStep.tsx
  steps/ChooseTypeStep.tsx
  steps/SamlImportStep.tsx
  steps/SsoImportStep.tsx
  steps/AiStep.tsx
  steps/DoneStep.tsx
  AccountPicker.tsx      // shared checkbox tree used by BOTH import steps
src/renderer/wizard/createProfiles.ts  // shared name/slug/displayName logic (from handleConfirmImport)
```

**Do not rebuild Settings inside the wizard.** Steps should be thin presentational wrappers over
the same IPC calls Settings already uses (`createMasterPassword`, `setDefaultCredentials`,
`saveSettings`, `ssoListAccounts`, `fetchRoles`, `ssoCreateProfiles`). Some UI duplication is
acceptable; duplicated *logic* is not.

Mounting: `App.tsx` already gates on `MasterPasswordState` before rendering
`PersistentMainContent`. The wizard mounts as a full-screen overlay *after* that gate resolves to
`unlocked`, so it never fights the unlock prompt.

## Visual treatment

- **No animation library.** `framer-motion` is a new dependency, and CLAUDE.md is explicit about
  dependency risk. Tailwind transitions plus the existing `animate-modal-in` keyframes cover
  step transitions, and CSS `@media (prefers-reduced-motion: reduce)` should disable them.
- Left progress rail with per-step icons in the existing inline-SVG style; completed steps get a
  check, skipped steps a muted dash so the user can see what they passed over.
- Slide + fade between steps (`translate-x` + `opacity`), ~200ms.
- The account picker gets per-account expand/collapse and a "select all" affordance — it is the
  step users will spend the most time on.

## Phases

| # | Scope |
| --- | --- |
| 1 | Extract `createProfiles.ts` + `AccountPicker.tsx` from the existing SSO import; no behavior change |
| 2 | SAML bulk import using them, exposed from the existing split-button menu |
| 3 | Wizard shell: step framework, progress rail, transitions, skip/back/next |
| 4 | First-run steps: welcome, master password, credentials, choose type, done |
| 5 | `setupCompleted` detection, Settings "Run setup again", import re-entry |
| 6 | Optional steps: terminal shell, Open WebUI |

Phases 1–2 are worth doing even if the wizard is never built — SAML bulk import is the biggest
standalone win here.

## Open questions

- Should the wizard ever be *mandatory*? Recommendation: no. It must always be skippable, and the
  app must be fully usable without it.
- Terminal-shell step: low value, and the setting auto-detects Git Bash already. Recommend dropping
  unless there is a reason to keep it.
- What should "both" do — run the SAML importer then the SSO importer back to back, or a single
  merged picker? Recommendation: sequential, since the two need different credentials.

## Resetting for testing

The app writes to **two** directories, which is itself worth knowing:

| Path | Contents |
| --- | --- |
| `%APPDATA%\AWSProfileManager` | `profiles.json`, `settings.json`, `sso-sessions.json`, `ui-prefs.json`, `rolesCache.json`, `auth-audit-log.json`, `startup-log.txt` |
| `%APPDATA%\aws-profile-manager` (dev) / `%APPDATA%\AWS Profile Manager` (packaged) | Electron `userData`: `Partitions/` (the `persist:sso-*` and `persist:console-*` sessions), `aws-cli-docs-cache/`, `Local Storage` (terminal layout), cookies, caches |

⚠️ The first path is hardcoded (`APP_NAME = 'AWSProfileManager'` in `profileStorage.ts` and
`settingsService.ts`) while the second follows Electron's app name. So **dev and packaged builds
share profiles and settings but NOT browser sessions or caches**. Worth reconciling separately.

Plus one Keytar entry: `AWSProfileManager/__default__` (the default IdP credentials).

A reset script lives at `scripts/reset-app-state.ps1`.

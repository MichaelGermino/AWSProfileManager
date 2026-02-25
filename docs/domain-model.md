# Domain model

## What an AWS Profile is in this system

- A **Profile** is an app-managed entity stored in `profiles.json` (see **profileStorage**). It is **not** read from `~/.aws/config` or `~/.aws/credentials`.
- **Fields** (from shared/types): `id`, `name`, `idpEntryUrl`, `label`, `autoRefresh`, `refreshIntervalMinutes`, `useDefaultCredentials`, `roleArn`, `principalArn`, `roleDisplayText`, `expiration`, `credentialProfileName`, `iconName`, `iconColor`. Legacy `accountNumber` exists but is deprecated in favor of `roleDisplayText` / `roleArn`.
- **credentialProfileName**: The section name used in `~/.aws/credentials` (e.g. `[myprofile]`). After a successful SAML assume-role, the app writes temporary credentials into that section. This is the name the user passes to `aws --profile &lt;credentialProfileName&gt;`.

## Profile list source

- **Profiles are not parsed from ~/.aws/config or ~/.aws/credentials.** The list of profiles is entirely defined by `profiles.json` in app data. The app creates/updates/deletes profiles in that file and, on profile delete, removes the corresponding section from `~/.aws/credentials` via `removeCredentialsSection(credentialProfileName)`.

## Abstraction layers

- **profileStorage**: Read/write `profiles.json`; normalize legacy `refreshIntervalHours` to `refreshIntervalMinutes` (min 60). Exposes getProfiles, saveProfile, deleteProfile, getProfileById, reorderProfiles, replaceAllProfiles.
- **credentialsFile**: Read/write `~/.aws/credentials` (INI) at `path.join(process.env.USERPROFILE || '', '.aws', 'credentials')`. Exposes readCredentialsFile, writeCredentialsForProfile, removeCredentialsSection. Used after STS assume-role and on profile deletion.
- **credentialStorage**: Keytar-backed storage for IdP username/password per profile (and default credentials). Key format: service `AWSProfileManager`, account profileId or `__default__`; password key for password, `{profileId}_username` for username. Renderer never receives passwords.

## Role assumption

- **Flow**: User triggers refresh → main loads IdP credentials from Keytar (if needed, renderer is prompted via `auth:credentialsRequired`) → axios logs into IdP (cookiejar) → GET IdP response containing SAML assertion → parse assertion (xml2js + regex fallback) for `Role` attribute (role_arn, principal_arn) → present roles to user via IPC → user selects role → main calls STS `AssumeRoleWithSAMLCommand` with assertion and selected role → on success, main writes `aws_access_key_id`, `aws_secret_access_key`, `aws_session_token` (and optionally region/output) to `~/.aws/credentials` under `credentialProfileName` via credentialsFile. Profile’s `expiration` and role fields are updated in profileStorage.
- **Roles cache**: rolesCache stores fetched roles by IdP entry URL (in app data) to avoid re-fetching when user only needs to pick a role again.

## Environment switching

- There is no “current profile” in the app’s main or renderer state beyond the **terminal screen**: the terminal has a profile dropdown; when the user inserts a command, the app can append `--profile &lt;credentialProfileName&gt;` if a profile is selected. The AWS CLI itself is run by the user in the terminal and reads from `~/.aws/credentials` and `~/.aws/config` according to `AWS_PROFILE` or `--profile`. The app does not set `AWS_PROFILE` in the PTY environment.

## Caching

- **Roles**: rolesCache (JSON file) keyed by idpEntryUrl; stores roles and fetchedAt.
- **AWS CLI docs**: awsCliDocsService caches parsed service list and per-service commands on disk (userData/aws-cli-docs-cache); TTL 24 hours. Fetches HTML via hidden BrowserWindow in main.
- **Profiles/settings**: No in-memory cache; read from disk on each IPC.

## File formats and parsing assumptions

- **profiles.json**: `{ profiles: Profile[] }`. UTF-8. JSON.parse; if malformed or missing, treat as `{ profiles: [] }`. Migration: `refreshIntervalHours` → `refreshIntervalMinutes` (×60), min 60.
- **~/.aws/credentials**: INI format. Parsed with `ini` package. Sections are profile names. Keys: `aws_access_key_id`, `aws_secret_access_key`, `aws_session_token`, optional `region`, `output`. Path: `process.env.USERPROFILE + '/.aws/credentials'` (Windows-oriented; USERPROFILE may be empty on non-Windows).
- **SAML**: Base64-decoded assertion parsed as XML (xml2js); Role attribute value is `role_arn,principal_arn` (or reverse). Regex fallback for ARN pairs in assertion body.

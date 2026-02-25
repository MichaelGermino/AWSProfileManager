# ADR 002: Credentials and API keys only in main process

## Context

- IdP passwords, AWS temporary credentials, and Open WebUI API key must not appear in the renderer (devtools, memory dumps, or IPC logs).
- Renderer needs to trigger auth flows and show “credentials required” / “refreshed” / “expired” and to request AI-generated CLI examples.

## Decision

- **IdP credentials**: Stored in Keytar (OS keychain) with service name `AWSProfileManager`. Retrieved only in main when performing SAML login. Renderer is never sent passwords; it sends username/password to main via IPC only when the user types them (e.g. in a credentials dialog). Main then calls Keytar set and proceeds with auth.
- **Open WebUI**: API URL and API key live in settings (main reads them). AI requests (generateAwsCliExample) are implemented in main; renderer invokes `ai:generate-cli` with only the prompt and receives command + explanation. Config status exposed as a boolean (configured or not), not the key or URL.
- **AWS credentials file**: Read and written only in main (credentialsFile). Renderer can request “open credentials file” (shell.openPath); it does not receive file contents.

## Alternatives considered

- **Sending API key to renderer for fetch**: Rejected; would expose key in renderer.
- **Storing IdP passwords in settings or profiles.json**: Rejected; Keytar used for OS-level protection.
- **Exposing “has credentials” per profile**: Accepted; boolean status is exposed for UI (e.g. “Save credentials” vs “Update credentials”) without exposing secrets.

## Consequences

- Any new feature that needs a secret (e.g. another API) must keep the secret in main and expose only a minimal status or result via IPC.
- Keytar is a native dependency; if it fails to load, credential storage is skipped and the app continues (no fallback to plaintext).
- Logging and error handling in main must avoid logging credential values or full request/response bodies for auth or AI.

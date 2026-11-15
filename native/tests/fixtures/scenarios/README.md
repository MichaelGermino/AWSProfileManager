# Backend scenarios

Shared behaviour tests for the 1.x Electron app and the 2.0 Rust backend
(`docs/native-rewrite-plan.md` §5.2). The same files run in both:

- Electron: `npx playwright test -c tests/parity` (after `npm run build`). Calls go through
  `window.electron.*`, mapped from channel names by `src/shared/ipcContract.ts`.
- Rust: `cargo test -p apm-core --test scenarios` calls `apm_core::dispatch(channel, args)`.

Electron is the reference. When a scenario and Electron disagree, Electron's behaviour is the
specification (unless it is a bug we explicitly decide to fix in both).

## File format

Each `.json` file holds one scenario or an array of them.

```json
{
  "id": "BE-AREA-NNN",
  "title": "One sentence describing the behaviour",
  "platforms": ["win32", "darwin"],
  "given": {
    "dataDir": "two-folders",
    "files": { "profiles.json": { "profiles": [] }, "raw.txt": "text as-is" },
    "credentials": "[dev]\naws_access_key_id=...\n"
  },
  "steps": [
    { "call": "profiles:getAll", "args": [], "expect": [], "save": "name" },
    { "restart": true }
  ],
  "then": {
    "files": { "profiles.json": { "json": {} }, "folders.json": { "absent": true } },
    "credentials": { "text": "[dev]{EOL}aws_access_key_id=...{EOL}" },
    "keychain": { "__default__": "$v1cipher", "p1": "$absent" }
  }
}
```

- `given.dataDir` copies `../datadirs/<name>/` into the app-data directory. `given.files` writes
  extra files (JSON values are written with 2-space indentation, strings as-is).
  `given.credentials` is the initial `~/.aws/credentials`.
- `call` is a channel from the IPC contract; `args` are its positional arguments. A handler that
  returns nothing yields `null`.
- `restart` closes the app and starts it again on the same data (persistence checks).
- `save` stores a step's result; `{"$ref": "name.field"}` reuses it in later `args` or `expect`.
- File paths in `then.files` are relative to the app-data directory. `credentials` is the
  credentials file. `keychain` lists entries under the test keychain service.

## Matching rules

| Expected value | Matches |
|----------------|---------|
| `"$any"` | anything |
| `"$string"`, `"$nonEmptyString"`, `"$number"`, `"$boolean"` | by type |
| `"$uuid"` | an RFC 4122 UUID string |
| `"$iso"` | an ISO-8601 timestamp string |
| `"$v1cipher"` | a master-password ciphertext (`v1:` prefix) |
| `"$absent"` (as an object value) | the key must not exist |
| object | same keys, each value matching; add `"$partial": true` to allow extra keys |
| array | same length, element-wise |
| anything else | strict equality |

In `text` expectations, `{EOL}` is the platform line ending. The `ini` package that writes the
credentials file in 1.x uses the OS line ending, so on Windows every line ends in CRLF.

## Behaviour already pinned down by these scenarios

- `profiles.json`, `folders.json` and `ui-prefs.json` are written as `JSON.stringify(x, null, 2)`:
  2-space indentation, **no trailing newline**, keys in insertion order (Rust: `serde_json` with
  `preserve_order`, and keep unknown fields).
- Reading never rewrites a file; a malformed file reads as empty and stays untouched.
- `refreshIntervalHours` (legacy) is converted on read; intervals clamp to a 60-minute minimum;
  the legacy key itself is kept.
- Rewriting the credentials file normalises every line ending to the platform EOL.

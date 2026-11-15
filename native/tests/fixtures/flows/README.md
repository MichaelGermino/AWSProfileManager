# UI flows

End-to-end user flows that run unchanged against the 1.x Electron app and the 2.0 GPUI app
(`docs/native-rewrite-plan.md` §5.1, layer C). A flow file has the same shape as a backend
scenario (`../scenarios/README.md`) — `given`, `steps`, `then`, ids — and may mix backend steps
(`call`, `expectEvents`, `dialogs`, ...) with the UI steps below. Flow ids start with `UI-`.

- Electron: `npx playwright test -c tests/parity` (same runner as the scenarios).
- GPUI: the native runner drives the built app through Windows UI Automation (the API screen
  readers use; proven in Phase 0 spike (e)) and reads the same files.

## Locating elements

Every element is found by **ARIA role and accessible name** — never by CSS class, text layout,
test id or coordinates. That is the one contract both apps can meet, and it doubles as an
accessibility requirement: if a flow cannot find a control, a screen-reader user cannot either.

```json
{ "role": "button", "name": "Add profile" }
{ "role": "button", "name": "$match:^Refresh " , "nth": 0 }
{ "role": "textbox", "name": "Profile name", "within": { "role": "dialog", "name": "New profile" } }
```

- `name` is the exact accessible name, or `"$match:<regex>"`. Omit it to match any name.
- `within` narrows the search to inside another element; `nth` picks one of several matches.
- `window` selects another window (`"logViewer"`); default is the main window.

### Role mapping for the GPUI runner

| Flow role | Windows UI Automation ControlType | Notes |
|-----------|-----------------------------------|-------|
| `button` | Button | Invoke pattern |
| `link` | Hyperlink | Invoke pattern |
| `textbox`, `searchbox` | Edit | Value pattern |
| `spinbutton` | Spinner ("spin button"), or Edit with RangeValue | |
| `checkbox` | CheckBox | Toggle pattern |
| `switch` | Button or CheckBox with Toggle pattern | AccessKit maps Switch to a toggle button |
| `combobox` | ComboBox | ExpandCollapse + Selection |
| `option` | ListItem | SelectionItem |
| `tab`, `tablist` | TabItem, Tab | SelectionItem |
| `heading` | Text with a heading level | |
| `dialog`, `alertdialog` | Window or Pane named by its heading | |
| `menu`, `menuitem` | Menu, MenuItem | |
| `list`, `listitem` | List, ListItem | |
| `navigation`, `main`, `complementary` | Group with the landmark type | |
| `status`, `alert` | Text / Group with live region | |
| `img` | Image | |

## UI steps

| Step | Meaning |
|------|---------|
| `{"click": L}` / `{"click": L, "button": "right"}` | Click (or right-click) the element |
| `{"hover": L}` | Move the pointer over it |
| `{"focus": L}` | Move keyboard focus to it (then `press` keys, e.g. Space/arrows to drag with the keyboard) |
| `{"hold": L, "ms": 1200}` | Press and hold (Space on the focused element; works for keyboard users) |
| `{"fill": L, "value": "..."}` | Replace the field's text |
| `{"select": L, "option": "label"}` | Choose an option of a combobox |
| `{"press": "Enter"}` | Press a key (Playwright key names) on the focused element |
| `{"type": "text"}` | Type text into the focused element |
| `{"see": L}` | The element is visible (waits up to the default timeout) |
| `{"see": L, "state": s}` | `visible`, `hidden`, `enabled`, `disabled`, `checked`, `unchecked`, `selected`, `expanded`, `collapsed`, `focused`, `pressed`, `unpressed` (aria-pressed; UIA TogglePattern On or Off) |
| `{"see": L, "count": n}` | Exactly `n` elements match |
| `{"see": L, "text": "..."}` | Its text contains this (or `$match:`) |
| `{"see": L, "value": "..."}` | A field's current value |

## Rules for writing flows

- Write what a person does and sees, in the order they do it; check results in the UI first and in
  files/keychain (`then`) second.
- Use the visible wording as the accessible name. When the redesign (unslop) changes wording, the
  flows change with it, once, and the GPUI app is built against the updated flows.
- Prefer one behaviour per flow; keep flows independent (each gets a fresh sandbox).

# Release notes

One Markdown file per release, named for the version in `package.json`:

```
docs/releasenotes/1.5.0-rc.5.md      ← for tag v1.5.0-rc.5
docs/releasenotes/1.6.0.md           ← for tag v1.6.0
```

The `v`-prefixed spelling (`v1.5.0-rc.5.md`) is accepted too.

## What happens on release

`.github/workflows/release.yml` looks for the file matching the tag being built and, if it
exists, sets it as the GitHub release body with `gh release edit --notes-file` — while the
release is still a draft, so it goes public with its notes already in place.

**No file is a warning, not a failure.** The release still ships; its body is just empty.

## Why this also drives the in-app popup

The app's "What's new" modal reads the **GitHub release body** for the version it is running
(`src/main/services/changelogService.ts`). So this file is the single source of truth: write the
notes here, and both the GitHub release page and the in-app popup show the same thing.

An empty body means the app shows nothing — that is the intended behaviour, not a bug. If you
ship without a notes file, no popup appears for that version.

## Writing them

Plain GitHub-flavoured Markdown. The in-app renderer supports headings, lists, tables, code
blocks, blockquotes, bold/italic and links (which open in the system browser). Images are
stripped, and raw HTML is escaped rather than rendered — so write Markdown, not HTML.

Keep the first heading descriptive; it is the first thing users see in the modal.

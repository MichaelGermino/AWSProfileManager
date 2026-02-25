# AWS Profile Manager – Documentation index

- **architecture.md** – System overview, main vs renderer, IPC, storage, error handling, security/performance notes.
- **tech-stack.md** – Electron, Node, React, Vite, node-pty, persistence, build, packaging.
- **domain-model.md** – Profile entity, storage (profiles.json; not from ~/.aws/config), credentials file, role assumption, caching.
- **security-model.md** – Credentials in memory, persistence, subprocesses, shell injection, logging, risks.
- **current-state.md** – Complete vs partial features, technical debt, what not to refactor casually.
- **adr/** – Architectural decision records (core architecture, credentials in main only).
- **ai-constraints.md** – Rules for future AI sessions: what not to change without updating what, credential/IPC/refactor constraints, patterns, logging, errors.

Use **ai-constraints.md** first when continuing development; use **architecture.md** and **security-model.md** before changing IPC or credential handling.

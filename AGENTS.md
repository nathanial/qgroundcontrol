# Agent Handoff Notes

- All active development now lives inside the `greenfield/` directory.
- Treat the existing Qt/C++ project as **read-only reference material** unless a task explicitly says otherwise.
- When adding code, tests, or docs, place them under `greenfield/` (or a subdirectory therein) so the rewrite stays isolated.
- Build/test commands should default to the greenfield toolchain (`npm run build`, `npm run dev`, Rust crates under `greenfield/`).
- If a request requires touching legacy files, double-check with the user before proceeding.

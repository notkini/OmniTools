# ADR 0001 — Stack and scaffolded versions

## Status

Accepted (Milestone 1, scaffolding day)

## Context

`PLAN.md` specified Tauri 2 + React + TypeScript (strict) + Vite + npm with
"current stable, compatible versions". The exact version numbers were to be
recorded here on the day of scaffolding, since they were not verifiable at
planning time (web search was unavailable).

## Decision

Scaffolded with `create-tauri-app@4.7.5` using the `react-ts` template, then
renamed all metadata from the scaffold project name to `omnitools`.

The versions actually locked by `npm install` on 2026-11-01:

| Package | Installed |
|---|---|
| `@tauri-apps/cli` | 2.12.4 |
| `@tauri-apps/api` | 2.12.4 |
| `@tauri-apps/plugin-opener` | 2.7.2 |
| `react` / `react-dom` | 19.3.1 |
| `typescript` | ~6.0.3 (installed 6.0.3) |
| `vite` | ^8.0.16 (installed 8.3.5) |
| `@vitejs/plugin-react` | 6.1.2 |
| Rust toolchain (pre-existing) | 1.99.1 stable, `x86_64-pc-windows-msvc` |

Tauri core crate: `tauri = "2"` (resolved at first `cargo` build; exact
version recorded in `src-tauri/Cargo.lock`).

## Conditions verified

- `@tauri-apps/cli` and `@tauri-apps/api` are on the same 2.x line (2.12.4). ✔ (PLAN §2.1 requirement)
- `tsconfig.json` has `"strict": true` and `"noUncheckedIndexedAccess": true` (added on top of template). ✔ (PLAN §12.9)
- `tauri.conf.json`: productName `OmniTools`, version `0.1.0`, identifier `io.github.aditya.omnitools`. ✔ (PLAN §12.7)

## Environment notes

- Rust: cargo is installed but the OpenCode session was launched with a
  stale environment snapshot that does not include `%USERPROFILE%\.cargo\bin`.
  The **user-level PATH already contains** `C:\Users\Aditya\.cargo\bin`
  (registry type REG_EXPAND_SZ, correct), so freshly started terminals
  (new VS Code windows / new sessions) resolve `cargo` normally.
  Session shells work around this by prepending the path per command.
- WebView2 Runtime 156.0 is present. ✔ (Tauri Windows prerequisite)

## Consequences

- Operations on Linux must be built on Linux (CI); no cross-compilation
  from Windows (PLAN §2.1).
- A minimal two-OS CI matrix (windows-latest + ubuntu-24.04) is added at
  Milestone 1 per PLAN §8 M1 task 6.

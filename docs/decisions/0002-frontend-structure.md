# ADR 0002 — Feature-oriented frontend structure

## Status

Accepted (user-approved restructuring task, before Milestone 2 UI work)

## Context

`PLAN.md` §3 originally proposed a `routes/registry/tools/shared`-style frontend
layout. Before any UI milestone begins, the structure was revised to a
conventional feature-oriented layout that scales as the tool suite grows. The
change was approved explicitly; this ADR records it and the mapping.

## Decision

Target frontend organization (`src/`):

| Directory | Purpose | Created |
|---|---|---|
| `app/` | Application shell only (App root, later Sidebar/TopBar/Palette/ThemeProvider) | ✅ now |
| `components/` | Generic, app-wide presentational components (`layout/`, `ui/`, `feedback/`) | when M2 lands UI primitives/shell pieces |
| `features/` | Vertical slices: `tool-registry/`, `search/`, `queue/`, `tools/<tool-id>/` (manifest + UI, self-contained) | when M3/M4/M5 land their code |
| `services/` | Typed `invoke()` wrappers over Rust commands | when M4 adds the typed fs layer |
| `shared/` | `hooks/`, `utils/`, `types/`, `constants/` (no UI) | when real shared code exists |
| `styles/` | `tokens.css`, `base.css`, design-system CSS | M2 |

Rules applied:
- **No empty placeholder files/directories.** Only `src/app/` and
  `src-tauri/src/commands/` exist today because they hold real content.
- Vite handles routing via the M3 registry; a `routes.tsx` will live in `app/`
  or at `src/` root when it has content.
- `main.tsx` remains the **single** frontend entry point at `src/main.tsx`.
- `vite-env.d.ts` stays at `src/` root (encodes Vite client types for the
  whole `src` include set; relocating is cosmetic).

Rust organization (`src-tauri/src/`), applied as content appears:
- `commands/` — thin IPC shells (one file per domain; currently only `greet`)
- `processing/` — pure, unit-tested transforms (M4+)
- `errors.rs` — serializable `AppError` model (M4+)
- `models.rs` — shared data types crossing IPC (M4+)
- `lib.rs` / `main.rs` — untouched entry points

## Mapping from the previous PLAN §3 layout

| Old location | New location |
|---|---|
| `src/app/*` shell pieces | `src/app/*` (unchanged intent) |
| `src/registry/*` | `src/features/tool-registry/*` |
| `src/tools/<tool>/` | `src/features/tools/<tool-id>/` |
| `src/shared/queue/store.ts` | `src/features/queue/` |
| `src/shared/ui/*` + `workspaces/ToolWorkspace.tsx` + `files/*` | `src/components/ui/*` (+ `layout/`, `feedback/` as appropriate) |
| `src/shared/fs/*` invoke wrappers | `src/services/` |
| `src/shared/hooks|utils|types|constants` | `src/shared/*` |
| `src/styles/*` | `src/styles/*` (unchanged) |
| `src-tauri error.rs` | renamed conceptually to `errors.rs` when created |

## Consequences

- Adding a tool = one folder under `features/tools/` + one line in the tool
  registry + one line in Rust `invoke_handler` (unchanged contract, plan §3).
- `PLAN.md` §3 tree updated to match this decision (recorded with reason).
- No dependency, capability, or build-config changes were needed.

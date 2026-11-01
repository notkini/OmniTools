# AGENTS.md — Persistent Instructions for OmniTools Desktop

These rules apply to **all future work** (human or AI-assisted) on the OmniTools Desktop project, on every new session, until formally revised. They take precedence over any conflicting instruction unless the user explicitly overrides them in the moment.

## Project context

- Free, open-source, offline-first desktop app for Windows and Linux: Tauri 2 + React + TypeScript (strict) + Vite + Rust + npm.
- The canonical roadmap lives in `PLAN.md`, including the six milestones, acceptance criteria, folder structure, design system, and MVP scope.

## Core rules

1. **Read `PLAN.md` before starting any milestone.** Re-check the Progress Tracker and the relevant milestone's tasks, verification steps, and acceptance criteria before writing code.
2. **Only implement the milestone explicitly approved by the user.** Do not begin the next milestone, or any part of it, without explicit approval.
3. **Preserve the minimal, polished, professional design direction defined in `PLAN.md` (Section 4).** No glassmorphism, no decorative gradients, no dashboard-style filler, no emojis as icons, no fake loading states. All new UI must compose from the design tokens and shared templates (`ToolWorkspace`, `tokens.css`, shared primitives) rather than inventing new visual language.
4. **Use TypeScript strict mode and follow the established Rust architecture.** `tsconfig.json` keeps `"strict": true` and `noUncheckedIndexedAccess`. Rust commands stay thin shells over pure, unit-tested functions in `processing/`; all file mutation happens in Rust, never in the web layer.
5. **Keep file processing local and offline-first.** Everything users drop into the app is processed on-device. No file ever leaves the machine.
6. **Do not add telemetry, mandatory cloud services, or unnecessary network requests.** No analytics, no accounts, no cloud upload. The only permitted outbound network action is an optional update check, off by default, if and when it is added.
7. **Use minimal Tauri permissions and validate file paths safely.** Grant only the capabilities actually needed (`dialog`, scoped usage, `core:event`; no `http`, no broad `shell`). Path handling goes through the central path-validation module (`paths.rs`) — reject symlink escapes, traversal, and out-of-root writes.
8. **Check dependency licences before adding dependencies.** Only MIT / Apache-2.0 / BSD / ISC / Zlib / Unicode families are acceptable. Never add GPL, AGPL, or unknown-licence dependencies (this includes GPL-family PDF libraries such as Poppler or MuPDF). Run the licence audit (cargo-deny / npm check) when it exists; do not bypass it.
9. **Never implement fake functionality.** No placeholder buttons that do nothing, no simulated processing results, no fabricated progress bars, no features that only appear to work. Every control must perform a real action and every result must come from real computation.
10. **Run relevant tests and build checks after changes.** At minimum: `npm run build` (tsc strict), `cargo clippy -- -D warnings`, and the relevant `cargo test` / `vitest` suites for the touched code before claiming any milestone is complete.
11. **Report actual test results and disclose failures honestly.** Never claim a feature works unless it has been implemented and verified. If a check fails, say so and show the error; do not de-scope silently to make a milestone "pass".
12. **Avoid unrelated changes and unnecessary dependencies.** Keep diffs small and reviewable; each change should serve the current approved milestone. Prefer pure-Rust or lightweight, well-maintained dependencies. registry `ToolDefinition` drives navigation, search, home screen, and routes — new tools touch only their own folder plus the registry import and the Rust `invoke_handler` array.
13. **Update `PLAN.md` when a milestone is completed or an approved technical decision changes.** Move the milestone's Progress Tracker status, and record the actual versions chosen (Milestone 1) or any decision change with a short "why" note. Never edit past acceptance criteria to make them easier to meet.
14. **Ask before making major architectural changes or expanding the approved scope.** This includes: new runtime dependencies beyond what `PLAN.md` lists for the current milestone, changing the folder-structure contract, altering the design system, granting additional Tauri capabilities, or pulling future-roadmap work into the present milestone.

## Quick reference per change

| Step | Check |
|---|---|
| Before | Read `PLAN.md`; confirm the approved milestone scope |
| During | Follow design system + architecture contracts; no unrelated edits |
| After | tsc / clippy / relevant tests pass; failures disclosed |
| Wrap-up | Update `PLAN.md` tracker or decisions; report real results |

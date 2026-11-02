# OmniTools Desktop — Implementation Plan

> **Project name:** OmniTools Desktop
> **Status:** Planned, not yet implemented
> **Purpose:** The canonical implementation roadmap for the project
> **Rule:** Complete and verify milestones individually before moving to the next.

Free, open-source, offline-first desktop application for Windows and Linux. Built with Tauri 2 + React + TypeScript (strict) + Vite + Rust. The MVP ships **4 tools**, not 8 — reliability, design quality, and an extensible foundation come first. Feature expansion happens via a tool registry so every new tool is purely additive.

---

## Progress Tracker

| Milestone | Description | Status |
|---|---|---|
| 1 | Environment and project validation | **Complete** (2026-11-02) — verified: toolchain, scaffold, dev + HMR, MSI/NSIS build + install/launch test, clippy/test/fmt/build locally; CI green on `windows-latest` and `ubuntu-24.04` for `bb7c750` (run 41432176973, all steps success; earlier run 41319754505 also green) |
| 2 | Application shell and design system | **In progress** — implemented and verified at code level (all checks green, shell launches, token gate clean); visual polish review pending user confirmation |
| 3 | Universal search and tool registry | **Complete** (2026-11-02) — registry single source of truth; in-app hash router (React Router deliberately skipped: one flat destination level, documented in `src/app/router.ts`); Ctrl+K/Ctrl+P palette with ARIA combobox + keyboard flows; About registered as first tool entry; scratch-tool exercise verified (new tool = its folder + registry import only); 33 vitest tests + tsc + clippy/fmt/test + token gate green; dev launch + HMR verified. Recents/favorites deferred to M4 (fake with one registered entry) |
| 4 | First fully functional utility (File Hasher) | **Complete** (2026-11-02) — committed at `7334ecb`; verified locally: cargo test (46 passed), cargo clippy/fmt clean, npm test (55 passed), npm run build clean, tauri dev launches. |
| 5 | Additional MVP tools (Image Converter, Batch Rename, PDF Merge/Split) | **In progress** — Image Converter implemented and locally verified: cargo test (63 passed total incl. 18 new imaging/save tests), cargo clippy/fmt clean, npm test (66 passed across 9 files), npm run build clean, tauri dev launches. Awaiting CI green on windows-latest + ubuntu-24.04 before marking complete. Batch Rename, PDF Merge/Split not started. |
| 6 | Testing, packaging, and release preparation | Not started |

---

## Implementation Rules

1. Read `PLAN.md` before starting a new milestone.
2. Implement only the currently approved milestone.
3. Do not start future milestones without explicit approval.
4. Run relevant tests and build checks before declaring a milestone complete.
5. Update `PLAN.md` when implementation decisions change, recording why.
6. Never claim a feature works unless it has been implemented and verified.
7. Prefer small, reviewable changes over large, unrelated code changes.
8. Preserve the established minimal, polished design direction (Section 4).
9. Do not add telemetry, mandatory cloud services, or unnecessary network requests.
10. Check dependency licences before adding new dependencies.

---

## Table of contents

1. [Environment findings](#1-environment-findings)
2. [Recommended technical architecture](#2-recommended-technical-architecture)
3. [Proposed folder structure](#3-proposed-folder-structure)
4. [Design-system specification](#4-design-system-specification)
5. [MVP feature comparison and final selection](#5-mvp-feature-comparison-and-final-selection)
6. [Dependency and licence considerations](#6-dependency-and-licence-considerations)
7. [Competitive differentiation](#7-competitive-differentiation)
8. [Milestone-by-milestone implementation plan](#8-milestone-by-milestone-implementation-plan)
9. [Repository and release preparation](#9-repository-and-release-preparation)
10. [Testing strategy](#10-testing-strategy)
11. [Major risks and mitigations](#11-major-risks-and-mitigations)
12. [Definition of done — Milestone 1](#12-definition-of-done--milestone-1)
13. [Recommended first implementation task](#13-recommended-first-implementation-task)

---

## 1. Environment findings

Workspace inspected before planning: `C:\Users\Aditya\Desktop\OmniTools` was **empty** — greenfield project. Environment verified:

| Component | Status |
|---|---|
| Rust | ✅ 1.99.1 stable, `x86_64-pc-windows-msvc` target — installed but **not on PATH** (lives in `~\.cargo\bin`) |
| Node.js | ✅ 24.12.0 |
| npm | ✅ 11.6.2 |
| Git | ✅ 2.51.0 |
| WebView2 Runtime | ✅ 156.0 (Tauri's Windows requirement, already satisfied) |
| Workspace | Empty directory (before `PLAN.md`) |

Note: web search was unavailable when this plan was drafted, so version numbers come from current knowledge and must be confirmed at Milestone 1 — the scaffolding step pulls latest stable versions automatically, so this is safe.

---

## 2. Recommended technical architecture

### 2.1 Stack evaluation — approved, with conditions

| Layer | Choice | Assessment |
|---|---|---|
| Shell | **Tauri 2.x** (latest stable in the 2.x line) | Correct choice. Produces a native-sized binary using the OS webview (WebView2 on Windows, WebKitGTK on Linux) instead of a bundled ~150 MB Chromium. Strong IPC, security model (capabilities/permissions), bundlers for NSIS/MSI/AppImage/deb/rpm, and a stable v2 plugin API. |
| UI | **React 19** + React Router (or TanStack Router) | Mature, large ecosystem, well-established patterns for lazy-loaded tool modules. |
| Language | **TypeScript 5.x, `"strict": true`** plus `noUncheckedIndexedAccess` | Non-negotiable for a registry-driven app where most plug-in safety comes from types. |
| Bundler | **Vite 7.x** | Standard for Tauri 2 templates; instant HMR keeps the AI-assisted loop fast. Works fine with Node 24. |
| Native | **Rust stable (1.99)**, `cargo` for everything native | Processing logic lives here: filesystem, hashing, image codecs, PDF. Testable without spawning the UI. |
| Pkg manager | **npm** | Fine. pnpm would save disk but adds friction with Tauri tooling for no real benefit here. |

**Conditions / compatible-version notes (verify at Milestone 1):**

- `@tauri-apps/cli` and `@tauri-apps/api` must be on the **same 2.x line**; mixing a v1 CLI with a v2 core is the #1 Tauri error.
- Tauri 2 plugins (dialog, fs, opener, updater) are versioned `^2` and must match core.
- Rust crates (`image`, `lopdf`, `sha2`, `walkdir`) were chosen partly for pure-Rust status — see Section 6.

**Windows-first appropriateness:** Excellent. The MSVC toolchain and WebView2 runtime are already present on this machine. Tauri's Windows story is its most mature; NSIS installer out of the box; no system dependencies beyond WebView2 (which the MSI/NSIS bootstrapper installs automatically on machines missing it).

**Linux distribution:** Architecturally sound, with one rule to accept: **you cannot cross-compile Tauri Linux binaries from Windows.** Linux builds (AppImage/deb/rpm) must be produced on Linux — a GitHub Actions `ubuntu-24.04` job solves this cleanly. Linux runtime needs standard `apt` packages (`webkit2gtk-4.1`, `libappindicator`, etc.) — documented in CI and CONTRIBUTING. Consequence: **run a minimal Linux CI build by Milestone 2 at the latest** so Linux breakage surfaces early, not at release time.

### 2.2 Runtime architecture

```
┌─────────────────────── WebView (React/TS) ───────────────────────┐
│  Tool Registry (single source of truth)                          │
│      → Sidebar / Home grid / Command palette / Router            │
│  Tool modules (lazy-loaded per tool: UI + option schema)         │
│  Shared UI kit (ToolWorkspace template, FileDropzone, …)         │
│  Processing queue store (zustand): batches, per-file state,      │
│      progress events, output-folder policy                       │
└────────────────────────── Tauri IPC ─────────────────────────────┘
┌────────────────────── Rust core (src-tauri) ─────────────────────┐
│  commands/: image.rs · hash.rs · rename.rs · pdf.rs              │
│      each command = thin IO shell over pure logic functions      │
│  processing/: pure, unit-tested transforms                       │
│  fs_paths.rs: scoped path validation, output-folder resolution   │
└──────────────────────────────────────────────────────────────────┘
```

Key rules:

- **All file mutation happens in Rust.** The web layer never writes files itself; it invokes commands. This gives one place for error mapping and one place to eventually sandbox/audit.
- **Every Rust command is a thin shell** over pure functions in `processing/` that take paths/bytes and return result types. Pure functions carry the unit tests.
- **Long-running work emits progress events** (per file + per batch) so the UI queue can render real, honest progress.
- **One batch queue model shared by all tools** — this is the UX differentiator (Section 7) and avoids each tool reinventing job state.

### 2.3 Tauri capabilities (permissions) — safe by default

- `core:window:default`, `core:event:default`.
- `dialog:allow-open` / `dialog:allow-save` (user-driven picks only — never programmatic browsing of arbitrary paths from the web layer).
- `fs` scoped: since the **Rust side** does the actual reads/writes via `std::fs` (Rust is not subject to the webview sandbox), the frontend needs only what it truly uses (e.g., `path` plugin, `opener` to reveal output in Explorer).
- Explicitly **not** granted: `http` (no fetch plugins), `shell` beyond `opener:reveal-item` + `opener:open-url` for the release page.
- Document in README: *"OmniTools performs zero network requests; the only outbound network action is an optional update check."* The update checker is an optional setting, **off by default** in the MVP.

---

## 3. Proposed folder structure

Monorepo-lite: one Tauri app. Flat and boring on purpose. The frontend layout was revised to a feature-oriented structure before Milestone 2 — see `docs/decisions/0002-frontend-structure.md`; directories appear only when real content exists, so futures listed below are the approved end-state.

```
OmniTools/
├─ .github/
│  ├─ workflows/
│  │  ├─ ci.yml              # fmt + clippy + tsc + vitest + cargo test, win + linux
│  │  └─ release.yml         # tauri-action → NSIS/MSI, AppImage/deb/rpm
│  ├─ ISSUE_TEMPLATE/
│  │  ├─ bug_report.yml
│  │  ├─ tool_request.yml    # "propose a new tool" template
│  │  └─ config.yml
│  ├─ CONTRIBUTING.md
│  └─ PULL_REQUEST_TEMPLATE.md
├─ LICENSE                    # Apache-2.0
├─ NOTICE                     # dependency licence inventory (generated + curated)
├─ README.md                  # privacy/offline statement up top
├─ docs/
│  ├─ ADDING_A_TOOL.md        # the contract any new tool must satisfy
│  ├─ PRIVACY.md              # exact network-behaviour statement
│  └─ decisions/              # short ADRs (Markdown) for key choices
├─ src/                       # React/TypeScript (feature-oriented, ADR 0002)
│  ├─ main.tsx                # SINGLE entry point; StrictMode; theme boot
│  ├─ app/                    # application shell only (owns nothing tool-specific)
│  │  ├─ App.tsx              # root component + co-located App.css
│  │  ├─ Sidebar.tsx
│  │  ├─ TopBar.tsx           # palette trigger, tool title, back
│  │  ├─ CommandPalette.tsx
│  │  ├─ ThemeProvider.tsx    # light/dark/system; persists
│  │  └─ routes.tsx           # maps tool registry → Router, lazy imports
│  ├─ components/             # generic presentational components, no business logic
│  │  ├─ layout/              # panels, resizable areas, page scaffolding
│  │  ├─ ui/                  # primitives: Button, IconButton, Kbd, Select, Tooltip…
│  │  │  └─ ToolWorkspace.tsx # THE reusable tool page template (dropzone/queue/options/actions)
│  │  └─ feedback/            # Toast, ProgressBar, inline per-file errors, EmptyState
│  ├─ features/               # vertical slices, each self-contained
│  │  ├─ tool-registry/       # ToolDefinition types + registry.ts (drives nav, search, home, routes)
│  │  ├─ search/              # command palette + fuzzy matcher
│  │  ├─ queue/               # shared batch queue store (zustand): run, cancel, progress events
│  │  └─ tools/               # one folder per tool (file-hash, image-convert, batch-rename, pdf-merge)
│  │     └─ file-hash/
│  │        ├─ manifest.ts    # ToolDefinition (id, name, category, icon, keywords, accepts, route)
│  │        ├─ HashTool.tsx   # workspace UI composed from components/ui + feedback
│  │        └─ index.ts       # re-export
│  ├─ services/               # typed invoke() wrappers over Rust commands (fs, dialogs, events)
│  ├─ shared/                 # hooks/, utils/, types/, constants/ — non-UI code only
│  ├─ assets/                 # bundled static images (vite-env.d.ts stays at src/ root)
│  └─ styles/
│     ├─ tokens.css           # design tokens (CSS variables)
│     ├─ base.css
│     └─ index.css
├─ src-tauri/
│  ├─ Cargo.toml
│  ├─ tauri.conf.json         # capabilities, bundle config, updater (disabled in MVP)
│  ├─ capabilities/default.json
│  ├─ icons/                  # squared assets; `tauri icon` generates all
│  └─ src/
│     ├─ main.rs              # builder, plugins, invoke handlers
│     ├─ commands/
│     │  ├─ mod.rs
│     │  ├─ hash.rs           # compute_sha256, verify
│     │  ├─ image.rs          # convert, resize, compress (per-file)
│     │  ├─ rename.rs         # plan_rename (dry run) + apply_rename (+ undo)
│     │  └─ pdf.rs            # merge, split
│     ├─ processing/          # PURE functions ← every unit test lives here
│     │  ├─ hashing.rs
│     │  ├─ imaging.rs
│     │  ├─ renaming.rs       # rename planner: collisions, pattern engine
│     │  └─ pdfops.rs
│     ├─ errors.rs            # serializable AppError model (IPC error codes)
│     └─ paths.rs             # path validation (traversal, symlink escape rejection)
├─ tests/
│  └─ fixtures/README.md      # explains how fixtures are generated deterministically
├─ .gitignore                 # node_modules, target/, dist/, src-tauri/gen, *.local
├─ package.json
├─ tsconfig.json              # strict, noUncheckedIndexedAccess, bundler resolution
├─ vite.config.ts
└─ index.html
```

### How a new tool is added (the contract, validated by M3/M4)

1. Create `src/features/tools/<id>/manifest.ts` implementing `ToolDefinition` — the registry does the rest (sidebar, search, home grid, route).
2. Create the tool UI by composing `ToolWorkspace` + shared file/queue components — no shell edits, no router edits.
3. If native processing is needed: add a `processing/*.rs` pure module + tests, then a thin command in `commands/*.rs`, and list it in the one `invoke_handler` array.
4. Nothing existing is touched except that array and `registry.ts`'s import line. `docs/ADDING_A_TOOL.md` codifies this, and a CI check (grep-based lint asserting every `src/tools/*` has a manifest) keeps it honest.

---

## 4. Design-system specification

**Direction:** quiet, dense-but-airy, monochrome-with-one-accent. The quality bar comes from **consistency and typography**, not decoration. Linear's reference points: calm surfaces, hairline borders, keyboard-native behaviour. Raycast's: the command palette as the spine, tight controls with generous whitespace, motion that explains rather than decorates.

### 4.1 Layout

```
┌────────────── Sidebar (232px, collapsible to 56px) ─┬── Main ─────────────────────┐
│  [logo wordmark]                                    │  TopBar (44px):             │
│  [search — opens palette · Ctrl K]                  │   back · tool title · ⋯     │
│  ───────────────                                    │─────────────────────────────│
│  Home                                               │                             │
│  Favorites ★                                        │   ToolWorkspace (below)     │
│  ─ Images                                           │                             │
│    Convert, Resize…                                 │                             │
│  ─ Documents                                        │                             │
│  ─ Files                                            │                             │
│  ─ ─────                                            │┌───────────────────────────┐│
│    Settings                                         ││  status bar (28px, when a ││
│  [collapse]                                         ││  batch ran): N ok · M err ││
└─────────────────────────────────────────────────────┴┴───────────────────────────┘┘
```

- Sidebar is `position: fixed`; content scrolls independently. Max content width ~960px for the tool workspace, centered; left-aligned when a queue demands width.
- **Command palette (Ctrl+K or Ctrl+P):** fuzzy match over the registry (tool names, category words, keywords), plus quick actions ("toggle theme", "open output folder", "quit"). Arrow keys + Enter. This is the spine of keyboard navigation; every tool is reachable in ≤2 keystrokes.

### 4.2 The tool workspace template (single most important reusable UI)

Every tool renders inside `ToolWorkspace` with the same regions — no tool may invent its own layout:

```
┌ Dropzone (when queue empty) ───── "Drop images or click to browse" ───┐
│   · accepted formats listed quietly beneath ("JPG · PNG · WebP · …")  │
└──────────────────────────────────────────────────────────────────────┘
[ FileQueue  ]  rows: icon · name · in-size → out-size · status
[ Options    ]  a right-side or below-queue panel; only tools' real options
[ Action bar ]  primary "Convert 12 files →" + output-location control
```

- **Output policy (default: "next to source")**: output beside originals with the new extension (`photo.jpg → photo.webp`), with a per-batch override to any chosen folder, and a Settings default. Avoids copy-litter (`-1.jpg` duplicates) while staying obvious.
- Convenience action in the status bar after success: **"Show in folder"** (single item) / "Open output folder" (batches).

### 4.3 Tokens (CSS custom properties in `tokens.css`)

**Color — light**

| Token | Value |
|---|---|
| `--bg` | `#fafafa` |
| `--surface` | `#ffffff` |
| `--surface-2` | `#f4f4f5` |
| `--border` | `#e5e5e5` (1px hairlines everywhere) |
| `--text` | `#171717` |
| `--text-2` | `#6f6f78` |
| accent | **one** deep indigo `#4f46e5` OR teal `#0d9488` — pick one |

The accent is used only for primary buttons, focus, and progress; everything else stays neutral.

**Color — dark** (authored, not auto-inverted): `--bg: #101012`, `--surface: #17171a`, `--border: #26262b`, `--text: #ededf0`. Borders lighten, shadows disappear, borders carry the structure.

- **Danger** `#dc2626` / **success** `#16a34a` used only in queue states and toasts.
- Theme via `[data-theme]` attribute + `color-scheme`; transitions 120ms; system-follow option.
- **Typography:** Inter (bundled, variable, `font-feature-settings: "tnum"` for sizes/hashes) or pure system stack (`Segoe UI Variable`, SF, Roboto) to avoid webfont weight — decide at M2; recommendation: system stack first, Inter as optional polish. Sizes: 13px base UI, 20px tool titles, 24px home wordmark region; line-height 1.45; hierarchy via size + weight, never via colour alone.
- **Spacing:** 4px scale (4/8/12/16/24/32/48). Sections separated by whitespace + hairlines, not cards.
- **Radius:** 6px controls, 10px dropzone/palette; no pill-heavy design; borders do the work.
- **Elevation:** light theme = `0 1px 2px rgba(0,0,0,.05)` max; dark theme = no shadows, surface-step contrast instead.
- **Icons:** lucide-react, 16px inline / 20px sidebar, stroke-width 1.75. No emojis, ever, anywhere.
- **Motion:** 100–140ms ease-out for hovers/focus, 180ms for palette in/out, queue rows animate in at 120ms stagger capped at 8 rows. `prefers-reduced-motion: reduce` disables all non-essential movement.

### 4.4 States (authored, not leftover)

- **Empty:** dropzone shows the accepted formats and a keyboard hint.
- **Loading:** per-file indeterminate → determinate progress with file counts, top-level `aria-busy`.
- **Success:** quiet toast — "Converted 12 files · 8.4 MB → 3.1 MB · Show in folder".
- **Error:** **inline per-file** in the queue row (red status + hover tooltip with the message), plus a summary line. No blocking modal errors mid-batch; the batch continues and reports honestly.

### 4.5 Anti-patterns (explicit build gates)

- No glassmorphism / backdrop-blur.
- No more than one gradient (zero preferred).
- No decorative cards with fake dashboard stats.
- No 999px-radius pills for structural elements.
- No fake loading skeletons for instant local operations.
- No emoji.
- No placeholder features that do nothing.

---

## 5. MVP feature comparison and final selection

| # | Tool | Native deps | Difficulty | Testing burden | Cross-platform issues | Notes |
|---|---|---|---|---|---|---|
| 1 | **Image conversion** | `image` crate (+ `libwebp-sys` if lossy WebP) | Medium | Low–medium (fixture images, per-format pixel assertions) | Minimal; lossy WebP pulls a small C build (BSD-3, fine) | Extensions must never be swapped; real transcodes only |
| 2 | **Image resize** | `image` (Lanczos3) | Easy | Low | None | Fold into the same tool as #1 |
| 3 | **SHA-256 hashing** | `sha2` (pure Rust) | Easy | Low (streaming chunks, known vectors) | None | Ideal first tool — proves the whole pipeline with trivial native code |
| 4 | **Batch rename** | std only | Easy–medium | High (planner: collisions, case-only renames on Windows, ordering) | Windows case-insensitivity edge cases; undo journal | Pure planner logic = great test target |
| 5 | **PDF merge** | `lopdf` (pure Rust, MIT) | Medium | Medium–high (real-world malformed PDF corpus) | None | lopdf fails honestly on exotic PDFs; surface clear per-file errors |
| 6 | **PDF split** | `lopdf` | Medium | Medium–high | None | Same story as #5; page-range selection UI |
| 7 | Image compression | `image` + `oxipng` for PNG; quality sliders for JPEG/WebP | Medium (overlaps #1) | Medium | Same as #1 | **Fold into converter** as a quality option (JPEG/WebP) and "optimize" for PNG |
| 8 | **PDF compression** | would need `pdfium` (C, Apache-2.0) or heavy stream surgery | Hard | High | Binary size +3–5 MB; fragile on odd PDFs | **Defer.** Real compression = recompressing embedded image streams or rasterization; too much for MVP |

### Final MVP scope (4 tools)

1. **File Hasher** (SHA-256, + compare two files / paste expected hash) — Milestone 4
2. **Image Converter** (PNG/JPEG/WebP + resize + JPEG/WebP quality & PNG optimization — candidate #7 collapsed into it) — Milestone 5
3. **Batch Rename** (patterns: prefix/suffix, numbering, case ops, extension ops; live preview with conflict highlighting; undo) — Milestone 5
4. **PDF tools: Merge** first; **Split** ships in the same milestone if merge lands cleanly, else moves to post-MVP — Milestone 5

Explicitly deferred (roadmap, Section 7): PDF compression, standalone compression tool, audio/video, archives, OCR.

**Why not all eight:** candidates 1+2+7 share one pipeline, #8 is the worst risk/reward in the list, and shipping 8 tools means 8 untested surfaces at once. Shipping 4 well-tested tools that share infrastructure proves the architecture — the stated success criterion of the first release.

---

## 6. Dependency and licence considerations

Our code: **Apache-2.0**, subject to dependency compatibility (Apache-2.0 is compatible with MIT/BSD/Apache; incompatible with GPL-only and encumbered licences).

### Rust side (all safe, mostly pure Rust)

| Crate | Licence | Note |
|---|---|---|
| `image` | MIT / Apache-2.0 | Decode/encode PNG/JPEG/GIF/BMP/TIFF; WebP **encode is lossless-only** in current versions |
| `libwebp-sys` | libwebp is **BSD-3** | For lossy WebP; C lib compiled via `cc` on MSVC — works, small; adds a C toolchain step to builds |
| `oxipng` | MIT | PNG optimization for the compression side |
| `lopdf` | MIT, pure Rust | Merge/split. Known limitation: exotic PDFs (XFA, unusual optimizers) can fail — handled as per-file errors, never a crash |
| `sha2` / `digest` | MIT / Apache-2.0 | Streaming SHA-256 |
| `walkdir`, `rayon`, `serde`, `thiserror` | MIT / Apache-2.0 | Batch parallelism (bounded workers), config serialization, error ergonomics |

**Avoid:** Poppler/cpp bindings (GPL), MuPDF (AGPL), any GPL-family PDF library. `pdfium-render` + the `pdfium` binary (Apache-2.0) is the sanctioned heavyweight for Phase 2 (render/extract/compress) — Apache-2.0 binary, +4–5 MB, vendored per-platform.

### Frontend (all MIT/ISC/Apache-friendly)

react, react-dom, zustand, lucide-react, react-router (or TanStack Router), vite, typescript, vitest.

- Keep UI component libraries to a minimum — the design system wants its own primitives (Section 4); use Radix primitives (MIT) if a11y-heavy pieces (popup/dialog) are needed.
- **Webfont policy:** system stack avoids shipping fonts (licensing + size); if Inter is chosen later it is OFL — fine, +~300 KB.

### Tooling and enforcement

- `cargo-deny` (MIT) enforcing an allow/deny licence list in CI: allow MIT / Apache-2.0 / BSD-* / ISC / Unicode; deny GPL / AGPL / unknown.
- npm licence check via a lightweight script (e.g., `license-checker` or manual review of the short dependency tree). Output: generated `NOTICE` additions reviewed per release. Both wired to CI as gates.

### Security / offline

- All processing is local; no fetch plugins granted in capabilities; no telemetry (no analytics at all — documented in `docs/PRIVACY.md`).
- Rust `std::fs` + a path-validation module (`paths.rs`) rejects symlink escapes and out-of-roots writes; behaviour is deterministic.
- Updater disabled by default.

---

## 7. Competitive differentiation

Reference competitor: **MiniMax Converter** (broad tool count, web-centric origins). We will not compete on feature count — the registry architecture makes feature count cheap to match over time, but it cannot be the launch story. Credible hypotheses, each with its validation signal:

| # | Hypothesis | Concrete plan | How we'd know it's working |
|---|---|---|---|
| 1 | Privacy transparency is a deciding factor for desktop tools | README + in-app Privacy panel listing every capability the app has and every network call (zero by default); visible "works fully offline" badge in About | Issue/PR chatter citing privacy; user interviews |
| 2 | Consistency beats breadth | One ToolWorkspace template, one queue, one error model app-wide — tools *feel* like one product | Qualitative feedback; churn between tools vs the rival's mixed web UI |
| 3 | Keyboard-first attracts power users | Ctrl+K palette drives everything; full keyboard flows for every tool including rename preview | Palette usage telemetry (local-only, opt-in stats in settings, default off) |
| 4 | Batch UX is poorly served everywhere | First-class queue with per-file status, bounded parallel workers, honest progress, one shared model so every tool gets it free | GitHub issues tagged "batch" going to zero |
| 5 | Easy install and a portable option | NSIS + MSI + portable .exe zip for Windows; AppImage/deb/rpm for Linux; signed once infra is stable | Download numbers and issue volume around install |
| 6 | Extensible open ecosystem | `ADDING_A_TOOL.md` + "tool request" issue template; tools are self-contained modules; later: third-party local tool manifests (post-MVP) | External PRs landing tool modules; RFC discussions |
| 7 | Reliability of output (no silently corrupted conversions) | Per-file success/failure with reason, deterministic tests on fixture corpora, "verify" mode in the Hasher | Bug reports as a % of tool usage; user trust quotes |

Explicitly *avoided* differentiation: fake progress bars, cloud-sync upsell, accounts, ads, "AI everything" — none of it belongs in the MVP, and half of it poisons the trust positioning.

### Future roadmap (post-MVP phases)

- **Document tools:** more PDF ops (reorder pages, rotate, extract text/images via `pdfium-render`), plain-text/CSV utilities. DOCX/ODF processing in pure Rust is weak — flag as needing careful dependency selection.
- **Advanced image operations:** crop, watermarking, format additions (AVIF via `ravif`), metadata inspector.
- **Audio/video:** requires **FFmpeg** (external executable, ~100 MB, LGPL/GPL provenance must be carefully chosen — LGPL build recommended). Large dependency; optional download-on-demand or bundled variant decision deferred.
- **Archive utilities:** zip via pure Rust (`zip` crate) first; 7z/rar need external tools or encumbered libraries — **unrar's licence is not OSI-approved; avoid**; treat 7-Zip integration as an external-binary decision.
- **OCR (optional advanced feature):** Tesseract (Apache-2.0) + language model downloads, or onnxruntime + model downloads — an explicit opt-in "feature pack" with consent; never in MVP.
- **Third-party tool manifests:** local, opt-in extensibility after the built-in contract is proven.

Rule of thumb: features requiring large dependencies, external executables, or model downloads become **opt-in feature packs** with explicit consent and documentation, keeping the core install small and fully offline.

---

## 8. Milestone-by-milestone implementation plan

### Milestone 1 — Environment & project validation (est. 0.5–1 session)

**Goal:** prove the toolchain end-to-end before any product code.

**Tasks:**

1. Add `%USERPROFILE%\.cargo\bin` to PATH (user environment variables) so VS Code/OpenCode shells see `cargo`; verify with `cargo --version` in a fresh shell.
2. Initialize the git repo; add `.gitignore` (node_modules, dist, target, src-tauri/gen, .DS_Store, *.local).
3. Scaffold: `npm create tauri-app@latest omnitools -- --template react-ts --manager npm --yes`, then move contents into the workspace root (or scaffold in-root). Record the actual stable versions in `docs/decisions/0001-stack.md`.
4. `npm install`, then `npm run tauri dev` — window opens; HMR verified.
5. `npm run tauri build` — NSIS installer produced under `src-tauri/target/release/bundle/`.
6. Add baseline CI (`.github/workflows/ci.yml`): matrix `windows-latest` + `ubuntu-24.04` running `npm ci && npm run build && cargo clippy -- -D warnings && cargo test` (the Linux job installs webkit2gtk system deps) — **first Linux signal early, as required by Section 2**.
7. Set `productName`, version `0.1.0`, identifier `io.github.<user>.omnitools` in `tauri.conf.json`; add `LICENSE` (Apache-2.0) and a blank `NOTICE`.

**Files:** `tauri.conf.json`, `Cargo.toml`, `package.json`, `tsconfig.json`, `vite.config.ts`, `.gitignore`, CI workflow, LICENSE/NOTICE, `docs/decisions/0001-stack.md`.

**Verification:** fresh VS Code terminal runs all version checks; `tauri dev` hot-reloads a text change; `tauri build` installer installs and launches on this machine; CI green on both OSes.

**Acceptance criteria:** all of the above checked; repo committed with clean `git status`; toolchain documented in `docs`.

**Risks:** cargo PATH mismatch (this machine's actual gap — fix first); Node 24 vs older templates (re-run scaffold if needed); Linux CI failures from missing apt deps (expected, fix iteratively); first Tauri build is slow (cold, ~5–10 min — fine).

### Milestone 2 — Application shell & design system (est. 1–2 sessions)

**Goal:** the app *looks and feels finished* before a single tool exists.

**Tasks & files:**

1. `styles/tokens.css` + `base.css` — full token set (Section 4); both themes authored.
2. `app/` — `App.tsx` layout grid, `Sidebar.tsx` (categories from an empty registry), `TopBar.tsx`, `ThemeProvider.tsx` (light/dark/system, persisted; Tauri store plugin optional later).
3. `shared/ui/` primitives: `Button`, `IconButton`, `Kbd`, `Tooltip`, `Toast` (+ queue), `Select`, `TextInput`, `EmptyState`, `ProgressBar`. Small, hand-rolled, no UI framework — keeps the design system proprietary and dependency-free.
4. `shared/workspaces/ToolWorkspace.tsx` template with dropzone/queue/options/action-bar regions; `shared/files/` — `FileDropzone`, `FileQueue`, `OutputLocation` — composed yet empty: rendering home with a well-designed "first tool coming" empty state.
5. Accessibility pass: focus rings, `:focus-visible`, skip links, `aria-current` on nav, reduced motion.
6. Optionally wire vitest with 2–3 trivial component/theme tests to prove the test harness.

**Verification:** manual checklist (theme toggle, responsive breakpoints 1200/900/640, keyboard-only walkthrough, reduced motion); Linux CI still green.

**Acceptance:** sidebar + home + settings-only app is coherent and shippable-looking; zero design tokens hardcoded outside `tokens.css` (grep check).

**Risks:** WebKitGTK rendering differences (flex/grid quirks) on Linux — optional CI screenshots; scope creep into "design everything" — the template pattern exists to stop this.

### Milestone 3 — Tool registry, routing, universal search (est. 1 session)

**Goal:** the plugin skeleton, proven with real navigation mechanics.

**Tasks & files:**

1. `registry/types.ts` — `ToolDefinition { id, name, description, category, icon, keywords, routePath, accepts?, Component (lazy) }`.
2. `registry/registry.ts` — collects manifests; derives: sidebar sections, search index, route table, home grid categories.
3. `routes.tsx` — Router mapping registry → React Router lazy routes.
4. `app/CommandPalette.tsx` — Ctrl+K / Ctrl+P fuzzy search (hand-rolled scored matcher, no heavy dependency), keyboard flows, recent-tools ring.
5. `home/Home.tsx` — favorites (star → persisted), recents, category browsing; empty states done right.
6. Register a real About page as the first registry entry, exercising Template + registry end-to-end.
7. Settings screen: theme, default output-folder policy, "Privacy: what this app can do" page.
8. `docs/ADDING_A_TOOL.md` written from real experience of M3.

**Verification:** a new tool added in a scratch branch — touches only its folder + 2 import lines; palette finds everything; keyboard-only walkthrough; unit tests for fuzzy matcher + registry invariants (unique ids, valid categories).

**Acceptance:** adding a tool requires zero edits to shell/route/nav code beyond registration; search covers every tool in ≤2 keystrokes.

**Risks:** over-engineering the registry (metadata bloat) — cap it with the "actually used by nav/search/home grid" rule.

### Milestone 4 — First fully functional utility: File Hasher (est. 1–2 sessions)

**Why first:** trivial pure-Rust core (`sha2`) → the entire pipeline (drop → queue → Rust command → progress events → results → output affordances → error states) gets exercised with near-zero algorithmic risk.

**Tasks & files:**

1. `src-tauri/src/processing/hashing.rs` — streaming SHA-256, pure, unit tests with official NIST vectors; `commands/hash.rs` — `hash_files(paths) -> Vec<FileOutcome>`, `compare_files(a, b)`, progress emitted per file (bounded workers via `std::thread`, capped at `MAX_CONCURRENCY=4`; Rayon was considered but std::thread was used to avoid an extra dependency).
2. `src/features/tools/file-hash/manifest.ts` + `HashTool.tsx` — compose ToolWorkspace: dropzone (any file), queue with per-file hash bytes → hex, compare panel (file vs pasted/file), copy-to-clipboard per row, "verify" state (match/mismatch visuals).
3. `src/shared/queue/store.ts` finalized here — enqueue, run, cancel, aggregate progress, per-file states (`idle | running | done | error`), events subscription; `src/shared/fs/` typed invoke wrappers mapping the Rust `AppError` enum to codes the UI renders.
4. `src-tauri/src/error.rs` — the canonical serializable error model (used by every later command). Named `error.rs` (singular) rather than `errors.rs` as originally drafted.
5. `src-tauri/src/paths.rs` — path validation (traversal, symlink escape rejection) per AGENTS.md rule 7. Does not restrict user-selected files to an application root.
6. Output affordances: "copy all hashes to file" writes `checksums.txt` via the save dialog.

**Testing:** Rust unit tests (NIST vectors, large-file streaming chunking, 0-byte file), command-level integration via direct function calls; TS vitest for the queue store with a mocked-invoke adapter (proves the UI state machine without launching Tauri).

**Verification:** 1000-file batch cancels cleanly and completes; hash of a known test file matches `certutil -hashfile <f> SHA256` on Windows and `sha256sum` on Linux CI; power loss mid-run leaves no partial artifacts.

**Acceptance:** tool fully usable end-to-end in both themes, keyboard-navigable, batch-tested at 1/100/1000 files, CI green on Windows + Linux, zero `unwrap()`s in command paths.

**Risks:** none technical; the discipline risk is polishing UI forever instead of moving to M5 — the acceptance criteria above are the fence.

### Milestone 5 — Additional MVP tools (est. 3–5 sessions)

Order: **Image Converter → Batch Rename → PDF Merge → (optionally) PDF Split.**

**1) Image Converter (convert + resize + quality/compress option)**

- `processing/imaging.rs`: decode via the `image` crate (PNG/JPEG/WEBP/GIF/BMP/TIFF in), encode out (PNG/JPEG/WebP). Resize via `imageops::resize` (Lanczos3) only when dimensions are requested; EXIF orientation handled; **strip metadata by default, checkbox to keep** (privacy story; document it).
- `commands/image.rs`: per-file `convert_one(in, out, opts)`; batch orchestration reuses the M4 queue; progress + per-file errors.
- UI: format select, quality slider (0–100, JPEG/WebP), optional resize (width/height presets, "longest side", keep aspect), PNG optimize toggle (`oxipng`), keep-metadata toggle, output naming (`photo.webp` beside source or chosen folder).
- Tests: round-trip pixel assertions on tiny fixtures (32×32 solid colour, gradient, alpha) — generated in Rust tests rather than committing binaries; per-format EXIF-strip verification; resize dimension correctness incl. odd aspect ratios.
- **WebP lossy decision gate:** try `libwebp-sys` early in a throwaway build; if MSVC C-build friction appears, ship WebP-lossless only and note the limit — do not block the milestone.

**2) Batch Rename**

- `processing/renaming.rs` — **pure planner**: `plan(operations, inputs) -> Vec<RenameOp> { from, to, kind: ok | conflict | unchanged }`. Operations: prefix/suffix, replace/find, numbering (start, step, padding), case transforms (UPPER/lower/Title), extension ops, pattern tokens (`{n}`, `{date}`, `{orig}`).
- `commands/rename.rs`: `plan_rename` (dry run) and `apply_rename` with an **undo journal** (in-memory + optional `.omni-undo` file in the target dir); collision detection (including case-only collisions **specific to Windows** — renaming `A.txt` ↔ `a.txt` needs a two-step swap); invalid-character sanitization per platform; reserved names (`CON`, `NUL`) on Windows.
- UI: file list, operation builder (stacked, reorderable), **live preview table** (old → new, conflicts red with reasons and "skip" toggles), Apply + Undo.
- Tests: the crown jewel — enumerate planner edge cases: collisions, duplicate targets, empty results, unicode names, Windows-reserved names, case swaps, sorted-order stability; undo restores exactly.

**3) PDF Merge (and Split if confident)**

- `processing/pdfops.rs` via `lopdf`: `merge(files) -> merged path`; per-file failure modes mapped to honest errors (encrypted, corrupt, unsupported) without aborting the batch (skip + report).
- `commands/pdf.rs`; UI with drag-to-reorder file list; quality of life: show per-file page counts (needs `lopdf` page read; fallback "unknown" without failing the row).
- Split: page ranges (`1-3,5,8-`), extract-to-separate-files toggle.
- Tests: generate simple multi-page PDFs in tests via `lopdf` itself or committed minimal fixtures; merge produces the correct page count and opens in a viewer; split boundaries exact.
- **Licence note:** `lopdf` is MIT — clean. Never pull GPL-family PDF libs (e.g., Poppler).

**Verification (each tool):** fixture-based Rust tests; per-tool CI run on both OSes; manual test on Windows 11; the "adding-a-tool cost" audit — confirm each new tool touched only its own folder + `registry.ts` + command registration.

**Acceptance:** four tools usable in batch, honest state machines everywhere, per-file errors with reasons, undo works, CI matrix green, `docs/ADDING_A_TOOL.md` updated with lessons learned.

**Risks:** lopdf edge failures on real-world PDFs (mitigate: explicit per-file errors + curated test corpus; an optional `pdfium-render` fallback planned for Phase 2); WebP lossy C-build friction (feature-gate it); scope creep via "one more small option" (fence: options not in the spec wait for post-MVP, tracked in issues); Tauri `fs` capability misconfiguration (test on Linux CI as we go).

### Milestone 6 — Testing, packaging & release preparation (est. 2–3 sessions)

**Tasks:**

1. **Licence audit** — `cargo-deny` (deny GPL/AGPL/Unknown; allow MIT/Apache-2.0/BSD/Zlib/ISC/Unicode), npm licence script, regenerate `NOTICE`; both wired to CI as gates.
2. **Hardening pass** — capability file audit (minimum permissions); `paths.rs` adversarial tests (symlinks, `..` traversal, UNC paths, long paths on Windows, permission-denied dirs); fuzz-lite: malformed inputs for `pdfops`/`renaming` from a corpus (tiny committed fixtures, generated where licences require).
3. **Performance sanity** — a 200-image batch completes in reasonable time; the UI thread is never blocked (all work in Rust); memory profile on a 2000-file hash batch.
4. **Packaging** — `tauri.conf.json` bundle config: Windows NSIS + MSI (+ portable zip of the single .exe), Linux AppImage/deb/rpm; updater **disabled**, documented as future work; version `0.1.0`, tag `v0.1.0`.
5. **Release workflow** — `.github/workflows/release.yml`: tag-triggered, matrix builds, artifact staging + GitHub Release with SHA-256 checksums of the artifacts; first release prerelease-tagged.
6. **Documentation** — README (privacy statement first, offline guarantee, install matrix, screenshots), CONTRIBUTING, `PRIVACY.md`, issue templates (bug / tool request), PR template.
7. **Polish sweep** — design-system audit against Section 4.5; accessibility pass; final keyboard-only walkthrough.

**Verification:** fresh Windows VM (or second machine) installs via NSIS cleanly (no WebView2 → auto-bootstrap works); Linux install via AppImage on Ubuntu 24.04; everything installable offline; installer runs with no network calls (verify via a firewall monitor or absence of connections during use).

**Acceptance criteria (release gate):** CI green with the licence gate; app launches and all 4 tools work on Windows + Linux; zero network egress confirmed; documentation complete; `v0.1.0` prerelease published with artifacts + checksums.

**Risks:** Linux packaging intricacies (AppImage tooling, desktop file, icon sizes) — budget iteration time; installer signing deferred (document why + a SmartScreen note for users); WebView2 bootstrap edge cases on old Windows 10 (document supported versions).

---

## 9. Repository and release preparation

- **Licensing:** Apache-2.0 for all original code (`LICENSE` at root), subject to dependency compatibility audits above; `NOTICE` maintained.
- **Dependency licence auditing:** `cargo-deny` + npm licence script as CI gates (Section 6).
- **Docs:** README (privacy/offline statement up top), CONTRIBUTING, `PRIVACY.md`, `docs/ADDING_A_TOOL.md`, ADRs in `docs/decisions/`.
- **GitHub:** issue templates (bug report, tool request), PR template, CI on PRs, tag-triggered release workflow.
- **Windows installation packages:** NSIS + MSI + portable zip.
- **Linux packaging:** AppImage + deb + rpm, built on CI.
- **Automated checks:** fmt, clippy (`-D warnings`), `cargo test`, `tsc`, vitest, licence gate.
- **Transparent documentation of offline behaviour:** zero network requests in core; optional updater off by default; every capability listed in the in-app Privacy panel.

---

## 10. Testing strategy

| Layer | Tool | What it covers | Owner of truth |
|---|---|---|---|
| Pure Rust | built-in `cargo test` | hashing vectors, rename planner matrix, image round-trips, PDF ops, path validation | `src-tauri/src/processing/*` |
| Command-level | `cargo test` (invoke fns directly, no IPC) | error mapping, batch semantics, cancellation | `src-tauri/src/commands/*` |
| TS logic | vitest | registry invariants, fuzzy matcher, queue store with a mocked-invoke adapter | `src/registry`, `src/shared/queue` |
| UI | manual checklists + optional Playwright (M6) | keyboard flows, themes, a11y, states | `app/`, `shared/ui` |
| System | CI matrix (windows / ubuntu) | toolchain, licence gate, packaging artifacts | `.github/workflows` |
| Fixtures | generated-in-tests + tiny committed binaries | avoid repo bloat / licence issues | `tests/fixtures` |

**Never-test-only-one-OS rule:** any file- or webview-touching feature must be exercised on both OSes in CI or in a manual Linux pass before a milestone closes.

---

## 11. Major risks and mitigations

| Risk | Severity | Mitigation |
|---|---|---|
| `cargo` not on PATH in VS Code terminal (confirmed on this machine) | M1 blocker | set user PATH in M1 step 1; document in README dev setup |
| lopdf fails on malformed real-world PDFs | Medium | per-file honest errors, curated corpus, optional `pdfium` path later |
| `libwebp-sys` MSVC C-build friction | Medium | gate WebP-lossy behind a cargo feature; fallback = WebP-lossless-only for MVP |
| Linux packaging churn discovered late | Medium | Linux CI mandatory from M1–M2; distribution trial runs by M6, not after |
| Scope creep and feature envy from the competitor | High | registry architecture + ADDING_A_TOOL contract makes additions cheap but *serialized*; MVP fence enforced in acceptance criteria |
| AGPL/GPL contamination via convenience deps | Medium | `cargo-deny` + npm licence script as CI gates from M3 onward |
| WebKitGTK rendering divergence | Low–Medium | design tokens kept simple (flex/grid/hairlines); optional Linux CI screenshots |
| Updater/signing complexity post-MVP | Low | explicitly deferred, documented as roadmap; prerelease honest about SmartScreen |
| AI-assisted drift (large unreviewed diffs) | Medium | small milestones with hard acceptance criteria; CI gates; docs as an artifact of implementation |

---

## 12. Definition of done — Milestone 1 (precise)

1. `cargo --version` succeeds from a **fresh** VS Code/OpenCode terminal (PATH fixed, user-level).
2. `C:\Users\Aditya\Desktop\OmniTools` is a git repo with an initial commit and no uncommitted changes.
3. Project scaffolded per the Section 3 folder skeleton (Tauri 2 + React + TS-strict + Vite + npm), with `npm run tauri dev` launching a window that reflects a saved-file change via HMR within 2 seconds.
4. `npm run tauri build` produces a Windows installer (NSIS or MSI) that installs and launches the stock app on this machine.
5. `npm run build` (TS strict, noUnusedLocals), `cargo clippy -- -D warnings`, and `cargo test` all pass locally.
6. `.github/workflows/ci.yml` runs the same checks on `windows-latest` **and** `ubuntu-24.04`, both green (Linux job includes webkit2gtk system deps).
7. `tauri.conf.json` has productName "OmniTools", version `0.1.0`, identifier `io.github.<user>.omnitools`.
8. `LICENSE` (Apache-2.0) + initial `NOTICE` committed; `docs/decisions/0001-stack.md` records the exact scaffolded versions chosen that day.
9. `tsconfig.json` has `"strict": true` and `noUncheckedIndexedAccess`.

---

## 13. Recommended first implementation task

**Milestone 1, exactly as scoped above — nothing else.** Concretely: fix the `cargo` PATH gap, `git init`, run `npm create tauri-app@latest` to scaffold the Tauri 2 + React-TS template into this directory, verify `tauri dev` hot-reload and the `tauri build` installer end-to-end, wire the two-OS CI workflow, and commit the `0001-stack.md` decision record with the actual pulled versions.

This is deliberately unglamorous: it de-risks every toolchain assumption in this plan within a single session, and produces the only artifact that can't be planned around — a running, building, CI-tested skeleton.

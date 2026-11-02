# OmniTools

Free, open-source, offline-first file utilities for Windows and Linux.
Everyday tools that run **entirely on your device** — no accounts, no uploads,
no telemetry. Drop your files in, get results out. Nothing ever leaves your
machine.

## Why OmniTools

Most "free" file tool websites are actually paywalls with ads, upload your
files to someone else's server, or bury one useful button under five popups.
OmniTools does the opposite:

- **100% local** — your files are processed on-device. No network requests.
- **Batch-first** — process one file or a hundred with the same flow.
- **Honest feedback** — real progress, real per-file errors when something
  fails. No fake spinners.
- **Keyboard-first** — a command palette (`Ctrl+K`) puts every tool one
  search away.
- **Open source (Apache-2.0)** — inspect the code, fork it, self-build it.

## What OmniTools will do

The suite is being built milestone by milestone. Planned tools:

| Tool | What it will do |
|---|---|
| **File Hasher** | SHA-256 checksums for single files or whole batches, with a verify mode to confirm nothing got corrupted |
| **Image Converter** | Convert between PNG, JPEG, and WebP, with resize and quality/compression options |
| **Batch Rename** | Rename hundreds of files with preview, collision detection, and undo |
| **PDF Merge / Split** | Combine PDFs or extract page ranges — without uploading a single page anywhere |

Everything ships with the same model: drag files in, watch each file's
status individually, and get a clear reason if one fails.

## Under the hood

- **Tauri 2** — a tiny native shell instead of a bundled Chromium
- **React + TypeScript (strict)** frontend
- **Rust** does all actual file processing
- Works offline after install; an optional update check would be **off by
  default** (not yet added)

## Building from source

```bash
npm install
npm run tauri dev      # run the desktop app in dev mode
npm run tauri build    # produce installers (MSI / NSIS)
npm test               # frontend unit tests
cargo test             # Rust tests (in src-tauri)
```

See [`docs/ADDING_A_TOOL.md`](docs/ADDING_A_TOOL.md) to add your own tool,
and [`PLAN.md`](PLAN.md) for the full roadmap.

---

## 🔨 Something's cooking…

The skeleton, navigation, and command palette are in. Now the real tools
are being forged:

```
- File Hasher        — in the forge
- Image Converter    — queued up
- Batch Rename       — planned
- PDF Merge/Split    — planned
```

Stay tuned — this is where the good stuff lands.

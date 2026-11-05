# Adding a tool to OmniTools

The tool registry (`src/features/tool-registry/`) is the single source of
truth: the sidebar, home screen, router, and command palette all derive from
it. If a step below requires editing those consumers, that is a bug in the
contract — please treat it as such.

This walkthrough is the real procedure used to register the first tool
(**About**), kept in `src/features/tools/about/`.

## Steps

1. **Create the tool folder** — `src/features/tools/<id>/`, where `<id>` is
   the stable kebab-case identifier (it doubles as the URL slug
   `/tools/<id>`). A tool folder contains:

   - the page component (default export, e.g. `About.tsx`);
   - `manifest.ts` exporting `const tool: ToolDefinition` (from
     `src/features/tool-registry/types.ts`);
   - an `index.ts` re-exporting `tool` from `./manifest`.

   ```ts
   // manifest.ts
   import { Wrench } from "lucide-react";
   import type { ToolDefinition } from "../../tool-registry/types";
   import MyPage from "./MyPage";

   export const tool: ToolDefinition = {
     id: "my-tool",
     name: "My Tool",
     description: "One line shown in Home rows and palette results",
     category: "files",            // must exist in the registry's label map
     icon: Wrench,
     keywords: ["short", "search", "words"],
     routePath: "/tools/my-tool", // always `/tools/${id}`
     load: async () => ({ default: MyPage }),
   };
   ```

2. **Register it** — add one import line and one entry to `TOOLS` in
   `src/features/tool-registry/registry.ts`:

   ```ts
   import { tool as myTool } from "../tools/my-tool";

   export const TOOLS: ToolDefinition[] = [aboutTool, myTool];
   ```

   Order in `TOOLS` is display order (sidebar, home, palette ties).

## What works automatically

- **Sidebar** entry under the Tools section, with active highlighting.
- **Home** list row (icon, name, description).
- **Route** at `/tools/<id>` with deep linking and back/forward support.
- **Command palette** match on name, keywords, category, description.
- **Unknown slugs** still fall through to the graceful Not Found page — no
  extra per-tool handling.

## If you need a new category

Extend `ToolCategoryId` in `types.ts` and add its label in `registry.ts`
(`CATEGORY_LABELS`). Do not pre-create categories without a tool using them.

## Rules tools must respect

- Type checks are strict (`tsc` + `noUncheckedIndexedAccess`); no `any`.
- All file processing happens in Rust commands, never in the web layer.
- Style with design tokens only (see `src/styles/tokens.css`); no hex
  colors outside `tokens.css`, no decorative gradients.
- Reuse shared primitives (`ToolWorkspace`, `Button`, `FileDropzone`, …)
  instead of inventing new visual language.

## Before you finish

- `npm test` (add tests for the tool's pure logic),
- `npm run build`,
- `cargo fmt --check && cargo clippy --all-targets -- -D warnings && cargo test`
  (if you added a Rust command), and register it in
  `src-tauri/src/lib.rs`'s `invoke_handler` array — the other Tauri/Rust
  surface a tool may touch.

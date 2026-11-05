import { useCallback, useEffect, useRef, useState } from "react";
import { Button } from "../../../components/ui/Button";
import { ToolWorkspace } from "../../../components/ui/ToolWorkspace";
import { useToast } from "../../../components/feedback/Toast";
import { pickRenameFiles, tauriInvokeAdapter } from "../../../shared/fs/invoke";
import { parseToolError } from "../../../shared/fs/errors";
import { OperationBuilder } from "./OperationBuilder";
import { RenamePreview, type RenameProposalRow } from "./RenamePreview";
import { toWireOperations, type RenameOperationDraft } from "./operations";

const PLAN_COMMAND = "plan_rename";
const APPLY_COMMAND = "apply_rename";
const UNDO_COMMAND = "undo_rename";

/** Wire shapes of the Rust outcome enums (camelCase tagged). */
export type RenameOutcome =
  | { kind: "renamed"; from: string; to: string }
  | { kind: "failed"; path: string; error: { code: string; message: string } }
  | { kind: "skipped"; path: string };

export type RenameUndoOutcome =
  | { kind: "restored"; from: string; to: string }
  | { kind: "failed"; from: string; error: { code: string; message: string } };

/** Rust planner + IPC round trip is ~instant locally; 250 ms coalesces typing. */
const PREVIEW_DEBOUNCE_MS = 250;

function formatValidationSummary(
  renamedCount: number,
  failedCount: number,
  skippedCount: number,
): string {
  const parts: string[] = [];
  if (renamedCount > 0) parts.push(`Renamed ${renamedCount}`);
  if (failedCount > 0) parts.push(`${failedCount} failed`);
  if (skippedCount > 0) parts.push(`${skippedCount} skipped`);
  return parts.length > 0 ? parts.join(" · ") : "Nothing to rename";
}

export default function BatchRenamePage() {
  const toast = useToast();
  const [paths, setPaths] = useState<string[]>([]);
  const [operations, setOperations] = useState<RenameOperationDraft[]>([]);
  /** Per-row skip control, keyed by absolute path (the Rust skip argument). */
  const [skipPaths, setSkipPaths] = useState<Set<string>>(new Set());
  const [proposals, setProposals] = useState<RenameProposalRow[]>([]);
  const [previewPhase, setPreviewPhase] = useState<
    "idle" | "loading" | "ready" | "error"
  >("idle");
  const [previewError, setPreviewError] = useState<string | null>(null);
  const [applying, setApplying] = useState(false);
  const [undoing, setUndoing] = useState(false);
  /** True only after a successful apply that actually renamed files — mirrors
   * the session-only undo journal in Rust (no query command exists, so this
   * optimistic flag is the honest proxy). */
  const [canUndo, setCanUndo] = useState(false);

  // Sequential request token: stale plan responses never overwrite newer ones.
  const planRequestRef = useRef(0);

  useEffect(() => {
    if (paths.length === 0) {
      planRequestRef.current += 1;
      setProposals([]);
      setPreviewPhase("idle");
      setPreviewError(null);
      return;
    }

    const requestId = ++planRequestRef.current;
    setPreviewPhase("loading");
    setPreviewError(null);

    const timer = setTimeout(async () => {
      try {
        const result = (await tauriInvokeAdapter(PLAN_COMMAND, {
          paths,
          operations: toWireOperations(operations),
        })) as RenameProposalRow[];
        if (requestId !== planRequestRef.current) return; // stale response
        setProposals(result);
        setPreviewPhase("ready");
      } catch (error) {
        if (requestId !== planRequestRef.current) return; // stale failure
        setPreviewError(parseToolError(error).message || "Preview failed");
        setPreviewPhase("error");
      }
    }, PREVIEW_DEBOUNCE_MS);

    return () => {
      clearTimeout(timer);
    };
  }, [paths, operations]);

  const addFiles = useCallback((newPaths: string[]) => {
    setPaths((prev) => {
      const existing = new Set(prev);
      const additions = newPaths.filter((path) => !existing.has(path));
      return additions.length > 0 ? [...prev, ...additions] : prev;
    });
  }, []);

  const toggleSkip = useCallback((path: string) => {
    setSkipPaths((prev) => {
      const next = new Set(prev);
      if (next.has(path)) {
        next.delete(path);
      } else {
        next.add(path);
      }
      return next;
    });
  }, []);

  const clearList = useCallback(() => {
    setPaths([]);
    setSkipPaths(new Set());
  }, []);

  const handleApply = useCallback(async () => {
    if (paths.length === 0 || applying) return;
    setApplying(true);
    try {
      const outcomes = (await tauriInvokeAdapter(APPLY_COMMAND, {
        paths,
        operations: toWireOperations(operations),
        skip_paths: [...skipPaths],
      })) as RenameOutcome[];

      // Outcomes are 1:1 with `paths`, in input order. Adopt the paths Rust
      // actually produced so the UI tracks the real filesystem state.
      const nextPaths = [...paths];
      let renamedCount = 0;
      let failedCount = 0;
      let skippedCount = 0;
      let anyRenamed = false;

      outcomes.forEach((outcome, index) => {
        if (outcome.kind === "renamed") {
          nextPaths[index] = outcome.to;
          renamedCount += 1;
          anyRenamed = true;
        } else if (outcome.kind === "failed") {
          failedCount += 1;
        } else {
          skippedCount += 1;
        }
      });

      setPaths(nextPaths);
      // Skips only remain meaningful for rows that kept their names.
      const stillSkipped = new Set<string>();
      outcomes.forEach((outcome, index) => {
        const original = paths[index];
        if (
          outcome.kind === "skipped" &&
          original !== undefined &&
          skipPaths.has(original)
        ) {
          stillSkipped.add(original);
        }
      });
      setSkipPaths(stillSkipped);
      setCanUndo(anyRenamed);

      toast(formatValidationSummary(renamedCount, failedCount, skippedCount), failedCount > 0 ? "error" : "success");
    } catch (error) {
      toast(`Rename failed: ${parseToolError(error).message || "unknown error"}`, "error");
    } finally {
      setApplying(false);
    }
  }, [paths, operations, skipPaths, applying, toast]);

  const handleUndo = useCallback(async () => {
    if (undoing) return;
    setUndoing(true);
    try {
      const outcomes = (await tauriInvokeAdapter(UNDO_COMMAND, {})) as RenameUndoOutcome[];

      // `Restored { from, to }`: the file was at `to` (our current path for it)
      // and now lives at `from` again — swap the frontend entry accordingly.
      const restoredTo = new Map(outcomes
        .filter((outcome) => outcome.kind === "restored")
        .map((outcome) => [outcome.to, outcome.from]));
      if (restoredTo.size > 0) {
        setPaths((prev) => prev.map((path) => restoredTo.get(path) ?? path));
      }

      const restoredCount = restoredTo.size;
      const failedCount = outcomes.filter((o) => o.kind === "failed").length;
      // Retryable failures stay in the session journal — keep Undo enabled
      // for them; a fully drained journal disables it honestly.
      setCanUndo(failedCount > 0);

      const parts: string[] = [];
      if (restoredCount > 0) parts.push(`Restored ${restoredCount}`);
      if (failedCount > 0) parts.push(`${failedCount} failed`);
      toast(parts.length > 0 ? parts.join(" · ") : "Nothing to undo", failedCount > 0 ? "error" : "success");
    } catch (error) {
      toast(`Undo failed: ${parseToolError(error).message || "unknown error"}`, "error");
    } finally {
      setUndoing(false);
    }
  }, [undoing, toast]);

  const canApply = paths.length > 0 && !applying;
  const actionLabel = applying
    ? "Renaming…"
    : `Rename ${paths.length} file${paths.length !== 1 ? "s" : ""}`;

  const optionsPanel = (
    <section className="rename-operations" aria-label="Rename operations">
      <h2 className="section-title">Operations</h2>
      <OperationBuilder operations={operations} onChange={setOperations} />
    </section>
  );

  const secondaryActions = (
    <>
      {paths.length > 0 && (
        <Button size="sm" onClick={clearList} disabled={applying}>
          Clear list
        </Button>
      )}
      <Button
        size="sm"
        onClick={() => void handleUndo()}
        disabled={!canUndo || undoing || applying}
      >
        {undoing ? "Undoing…" : "Undo"}
      </Button>
      <span className="workspace-note">
        Undo applies to the most recent rename this session. It is kept in
        memory only and is lost when OmniTools closes.
      </span>
    </>
  );

  return (
    <ToolWorkspace
      dropLabel="Add files"
      formatHint="Any file type"
      multiple
      onFiles={addFiles}
      onBrowse={pickRenameFiles}
      queue={[]}
      options={optionsPanel}
      actionLabel={actionLabel}
      onAction={() => void handleApply()}
      actionDisabled={!canApply}
      secondaryActions={secondaryActions}
      preview={
        <RenamePreview
          items={proposals}
          paths={paths}
          skipPaths={skipPaths}
          onSkipToggle={toggleSkip}
          disabled={applying || undoing}
          loading={previewPhase === "loading"}
          error={previewPhase === "error" ? previewError : null}
        />
      }
      previewAriaLabel="Batch rename preview"
    />
  );
}

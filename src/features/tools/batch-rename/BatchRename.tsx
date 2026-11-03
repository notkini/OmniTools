import { useCallback, useEffect, useState } from "react";
import { ChevronDown, ChevronUp, Minus, Plus, X } from "lucide-react";
import { Button } from "../../../components/ui/Button";
import { Select } from "../../../components/ui/Select";
import { IconButton } from "../../../components/ui/IconButton";
import { ToolWorkspace } from "../../../components/ui/ToolWorkspace";
import { useToast } from "../../../components/feedback/Toast";
import { createQueueStore } from "../../../shared/queue/store";
import {
  pickRenameFiles,
  tauriInvokeAdapter,
  tauriListenAdapter,
} from "../../../shared/fs/invoke";
import { parseToolError } from "../../../shared/fs/errors";
import { OperationBuilder } from "./OperationBuilder";
import { RenamePreview } from "./RenamePreview";
import { type RenameProposal } from "../../../src-tauri/src/processing/renaming";
import { type RenameOperationDraft } from "./operations";

const PLAN_COMMAND = "plan_rename";
const APPLY_COMMAND = "apply_rename";
const UNDO_COMMAND = "undo_rename";
const SAVE_COMMAND = "save_converted";
const SAVE_ALL_COMMAND = "save_all_converted";
const IMAGE_PROGRESS_EVENT = "omnitools://image-progress";
const IMAGE_SAVE_EVENT = "omnitools://image-save";

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

export default function BatchRenamePage() {
  const toast = useToast();
  const [paths, setPaths] = useState<string[]>([]);
  const [operations, setOperations] = useState<RenameOperationDraft[]>([]);
  const [skipPaths, setSkipPaths] = useState<Set<string>>(new Set());
  const [preview, setPreview] = useState<RenameProposal[]>([]);
  const [previewState, setPreviewState] = useState<
    "idle" | "loading" | "ready" | "error"
  >("idle");
  const [previewError, setPreviewError] = useState<string | null>(null);
  const [saveStates, setSaveStates] = useState<
    Map<string, "ready" | "saving" | "saved" | "saveError">
  >(new Map());
  const [applying, setApplying] = useState(false);
  const [undoing, setUndoing] = useState(false);

  const store = createQueueStore({
    invoke: tauriInvokeAdapter,
    listen: tauriListenAdapter,
    makeJobId: () => crypto.randomUUID(),
    command: PLAN_COMMAND,
    cancelCommand: UNDO_COMMAND,
    progressEvent: IMAGE_PROGRESS_EVENT,
  });

  const [snapshot, setSnapshot] = useState(() => store.getState());

  useEffect(() => {
    return store.subscribe(() => setSnapshot(store.getState()));
  }, [store]);

  const rows = snapshot.rows;

  const requestRef = useRef(0);

  useEffect(() => {
    if (paths.length === 0) {
      setPreview([]);
      setPreviewState("idle");
      setPreviewError(null);
      return;
    }

    const requestId = ++requestRef.current;
    setPreviewState("loading");
    setPreviewError(null);

    const timer = setTimeout(async () => {
      try {
        const wireOps = operations.map((d) => d.op);
        const result = (await tauriInvokeAdapter(PLAN_COMMAND, {
          paths,
          operations: wireOps,
        })) as RenameProposal[];

        if (requestId !== requestRef.current) return;

        setPreview(result);
        setPreviewState("ready");
      } catch (error) {
        if (requestId !== requestRef.current) return;
        const toolError = parseToolError(error);
        setPreviewError(toolError.message || "Preview failed");
        setPreviewState("error");
      }
    }, 250);

    return () => {
      clearTimeout(timer);
    };
  }, [paths, operations]);

  const handleFiles = useCallback(
    (newPaths: string[]) => {
      const existing = new Set(paths);
      const newPathsUnique = newPaths.filter((p) => !existing.has(p));
      setPaths((prev) => [...prev, ...newPathsUnique]);
    },
    [paths],
  );

  const handleRemovePath = useCallback(
    (path: string) => {
      setPaths((prev) => prev.filter((p) => p !== path));
      setSkipPaths((prev) => {
        const next = new Set(prev);
        next.delete(path);
        return next;
      });
    },
    [],
  );

  const handleConvert = useCallback(async () => {
    if (!paths.length || applying) return;

    const wireOps = operations.map((d) => d.op);
    setApplying(true);

    try {
      const result = (await tauriInvokeAdapter(APPLY_COMMAND, {
        paths,
        operations: wireOps,
        skip_paths: Array.from(skipPaths),
      })) as RenameProposal[];

      const newPaths = [...paths];
      let renamedCount = 0;
      let failedCount = 0;
      let skippedCount = 0;

      for (const outcome of result) {
        if (outcome.kind === "renamed") {
          const idx = newPaths.indexOf(outcome.from);
          if (idx !== -1) newPaths[idx] = outcome.to;
          renamedCount++;
        } else if (outcome.kind === "failed") {
          failedCount++;
        } else if (outcome.kind === "skipped") {
          skippedCount++;
        }
      }

      setPaths(newPaths);
      setSkipPaths((prev) => {
        const next = new Set(prev);
        for (const outcome of result) {
          if (outcome.kind !== "skipped") {
            next.delete(outcome.path);
          }
        }
        return next;
      });

      const summaryParts: string[] = [];
      if (renamedCount > 0) summaryParts.push(`Renamed ${renamedCount}`);
      if (failedCount > 0) summaryParts.push(`${failedCount} failed`);
      if (skippedCount > 0) summaryParts.push(`${skippedCount} skipped`);

      toast(summaryParts.join(" · "));
    } catch (error) {
      const toolError = parseToolError(error);
      toast(`Rename failed: ${toolError.message || "unknown error"}`);
    } finally {
      setApplying(false);
    }
  }, [paths, operations, skipPaths]);

  const handleUndo = useCallback(async () => {
    setUndoing(true);
    try {
      const result = (await tauriInvokeAdapter(UNDO_COMMAND, {})) as {
        kind: "restored" | "failed";
        from: string;
        to?: string;
        error?: { code: string; message: string };
      }[];

      const restoredPaths: string[] = [];
      const failedPaths: string[] = [];

      for (const outcome of result) {
        if (outcome.kind === "restored") {
          restoredPaths.push(outcome.from);
        } else if (outcome.kind === "failed") {
          failedPaths.push(outcome.from);
        }
      }

      setPaths((prev) =>
        prev.map((p) => {
          const restored = restoredPaths.find((r) => r === p);
          return restored ? p : p;
        }),
      );

      const restoredCount = restoredPaths.length;
      const failedCount = failedPaths.length;

      const summaryParts: string[] = [];
      if (restoredCount > 0) summaryParts.push(`Restored ${restoredCount}`);
      if (failedCount > 0) summaryParts.push(`${failedCount} failed`);

      toast(summaryParts.join(" · "));
    } catch (error) {
      const toolError = parseToolError(error);
      toast(`Undo failed: ${toolError.message || "unknown error"}`);
    } finally {
      setUndoing(false);
    }
  }, []);

  const handleSaveFile = async (staged_file_id: string) => {
    setSaveStates((current) => {
      const next = new Map(current);
      next.set(staged_file_id, "saving");
      return next;
    });
    const destination = await pickSaveDestination();
    if (destination === null) {
      setSaveStates((current) => {
        const next = new Map(current);
        next.set(staged_file_id, "ready");
        return next;
      });
      return;
    }
    try {
      await tauriInvokeAdapter(SAVE_COMMAND, {
        staged_file_id,
        destination,
      });
    } catch (error) {
      const toolError = parseToolError(error);
      toast(`Save failed: ${toolError.message || "unknown error"}`);
      setSaveStates((current) => {
        const next = new Map(current);
        next.set(staged_file_id, "saveError");
        return next;
      });
    }
  };

  const handleSaveAll = async () => {
    const destDir = await pickDirectory();
    if (destDir === null) return;
    const savable = rows.filter(
      (row) =>
        row.status === "done" &&
        row.staged_file_id &&
        saveStates.get(row.staged_file_id) === "ready",
    );
    if (savable.length === 0) return;
    const ids = savable.map((row) => row.staged_file_id!);
    ids.forEach((id) => {
      setSaveStates((current) => {
        const next = new Map(current);
        next.set(id, "saving");
        return next;
      });
    });
    try {
      await tauriInvokeAdapter(SAVE_ALL_COMMAND, {
        dest_dir: destDir,
        staged_file_ids: ids,
      });
    } catch (error) {
      const toolError = parseToolError(error);
      toast(`Save all failed: ${toolError.message || "unknown error"}`);
      ids.forEach((id) => {
        setSaveStates((current) => {
          const next = new Map(current);
          if (next.get(id) === "saving") {
            next.set(id, "saveError");
          }
          return next;
        });
      });
    }
  };

  const idleCount = rows.filter((row) => row.status === "idle").length;
  const savableCount = rows.filter(
    (row) =>
      row.status === "done" &&
      row.staged_file_id &&
      saveStates.get(row.staged_file_id) === "ready",
  ).length;
  const canConvert = paths.length > 0 && !applying;

  const convertLabel = applying
    ? "Renaming…"
    : `Rename ${paths.length} file${paths.length !== 1 ? "s" : ""}`;

  const optionsPanel = (
    <>
      <div className="workspace-section">
        <h3 className="workspace-section-title">Operations</h3>
        <OperationBuilder operations={operations} onChange={setOperations} />
      </div>
    </>
  );

  const secondaryActions = (
    <>
      {applying && (
        <button
          className="action-button action-button--secondary"
          onClick={handleUndo}
        >
          <X size={16} strokeWidth={1.75} aria-hidden="true" />
          Cancel
        </button>
      )}
      {!applying && paths.length > 0 && (
        <button
          className="action-button action-button--secondary"
          onClick={() => setPaths([])}
        >
          <RefreshCw size={16} strokeWidth={1.75} aria-hidden="true" />
          Clear list
        </button>
      )}
      {savableCount > 0 && !applying && (
        <Button size="sm" onClick={handleSaveAll} disabled={savableCount === 0}>
          Save all ({savableCount})
        </Button>
      )}
      {idleCount > 0 && (
        <span className="idle-count">
          {idleCount} file{idleCount !== 1 ? "s" : ""} ready
        </span>
      )}
    </>
  );

  return (
    <ToolWorkspace
      title="Batch Rename"
      description="Rename multiple files at once with find/replace, numbering, case change, and more"
      dropLabel="Add files"
      formatHint="Any file type"
      multiple
      onFiles={handleFiles}
      queue={rows.map((row) => ({
        id: row.id,
        path: row.path,
        name: row.name,
        status: row.status,
        size: row.sizeBytes ? formatBytes(row.sizeBytes) : undefined,
        staged_file_id: row.staged_file_id,
        outputPath: row.outputPath,
        message: row.message,
        save_state: row.staged_file_id ? saveStates.get(row.staged_file_id) : undefined,
      }))}
      onRemove={handleRemovePath}
      options={optionsPanel}
      actionLabel={convertLabel}
      onAction={handleConvert}
      actionDisabled={!canConvert}
      secondaryActions={secondaryActions}
    >
      {previewState === "loading" && (
        <div className="preview-panel">
          <div className="preview-empty">
            <span className="preview-empty-label">Computing preview…</span>
          </div>
        </div>
      )}
      {previewState === "ready" && preview.length > 0 && (
        <RenamePreview
          items={preview}
          skipPaths={skipPaths}
          onSkipToggle={handleRemovePath}
          disabled={applying}
        />
      )}
      {previewState === "error" && (
        <div className="preview-panel">
          <div className="preview-empty">
            <span className="preview-empty-label">
              Preview error: {previewError}
            </span>
          </div>
        </div>
      )}
    </ToolWorkspace>
  );
}

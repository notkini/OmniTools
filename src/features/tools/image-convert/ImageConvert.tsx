import { useCallback, useEffect, useState } from "react";
import { RefreshCw, X } from "lucide-react";
import { Button } from "../../../components/ui/Button";
import { Select } from "../../../components/ui/Select";
import { ToolWorkspace } from "../../../components/ui/ToolWorkspace";
import { useToast } from "../../../components/feedback/Toast";
import { createQueueStore } from "../../../shared/queue/store";
import {
  tauriInvokeAdapter,
  tauriListenAdapter,
  pickDirectory,
  pickSaveDestination,
} from "../../../shared/fs/invoke";
import { parseToolError } from "../../../shared/fs/errors";

const CONVERT_COMMAND = "convert_files";
const CONVERT_CANCEL_COMMAND = "image_cancel";
const SAVE_COMMAND = "save_converted";
const SAVE_ALL_COMMAND = "save_all_converted";
const IMAGE_PROGRESS_EVENT = "omnitools://image-progress";
const IMAGE_SAVE_EVENT = "omnitools://image-save";

type OutputFormat = "png" | "jpeg" | "webp";
type SaveState = "ready" | "saving" | "saved" | "saveError";

const FORMAT_LABELS: Record<OutputFormat, string> = {
  png: "PNG",
  jpeg: "JPEG",
  webp: "WebP",
};

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

export default function ImageConvertPage() {
  const toast = useToast();
  const [outputFormat, setOutputFormat] = useState<OutputFormat>("jpeg");
  const [quality, setQuality] = useState(85);
  const [resizeEnabled, setResizeEnabled] = useState(false);
  const [maxWidth, setMaxWidth] = useState("");
  const [maxHeight, setMaxHeight] = useState("");

  const store = createQueueStore({
    invoke: tauriInvokeAdapter,
    listen: tauriListenAdapter,
    makeJobId: () => crypto.randomUUID(),
    command: CONVERT_COMMAND,
    cancelCommand: CONVERT_CANCEL_COMMAND,
    progressEvent: IMAGE_PROGRESS_EVENT,
  });

  const [snapshot, setSnapshot] = useState(() => store.getState());
  const [saveStates, setSaveStates] = useState<Map<string, SaveState>>(new Map());

  useEffect(() => {
    return store.subscribe(() => setSnapshot(store.getState()));
  }, [store]);

  // Initialize save state to "ready" for newly completed conversions.
  useEffect(() => {
    const next = new Map(saveStates);
    let changed = false;
    for (const row of snapshot.rows) {
      if (
        row.status === "done" &&
        row.staged_file_id &&
        !next.has(row.staged_file_id)
      ) {
        next.set(row.staged_file_id, "ready");
        changed = true;
      }
    }
    if (changed) setSaveStates(next);
  }, [snapshot.rows, saveStates]);

  // Listen for save completion events from Rust.
  useEffect(() => {
    let unlisten: (() => void) | null = null;
    tauriListenAdapter(IMAGE_SAVE_EVENT, (payload: unknown) => {
      const p = payload as {
        stagedFileId?: string;
        kind?: "saved" | "saveError";
      } | null;
      if (!p?.stagedFileId || !p.kind) return;
      setSaveStates((current) => {
        const next = new Map(current);
        next.set(p.stagedFileId!, p.kind === "saved" ? "saved" : "saveError");
        return next;
      });
    }).then((u) => {
      unlisten = u;
    });
    return () => {
      unlisten?.();
    };
  }, []);

  const { rows, phase, totals } = snapshot;
  const isRunning = phase === "running";
  const hasFiles = rows.length > 0;
  const allIdle = rows.every((row) => row.status === "idle");
  const canConvert = hasFiles && allIdle && !isRunning;

  const convertLabel = isRunning
    ? "Converting…"
    : `Convert ${totals.total} file${totals.total !== 1 ? "s" : ""}`;

  const handleFiles = useCallback(
    (paths: string[]) => {
      store.addPaths(paths);
    },
    [store],
  );

  const handleConvert = useCallback(async () => {
    if (!hasFiles || isRunning) return;
    const resize =
      resizeEnabled && (maxWidth || maxHeight)
        ? {
            max_width: maxWidth ? Number(maxWidth) : 0,
            max_height: maxHeight ? Number(maxHeight) : 0,
          }
        : undefined;
    await store.run({
      opts: {
        output_format: outputFormat,
        quality: outputFormat === "jpeg" ? quality : undefined,
        resize,
      },
    });
  }, [
    hasFiles,
    isRunning,
    resizeEnabled,
    maxWidth,
    maxHeight,
    outputFormat,
    quality,
    store,
  ]);

  const handleCancel = useCallback(async () => {
    await store.cancel();
  }, [store]);

  const handleReset = useCallback(() => {
    store.reset();
    setSaveStates(new Map());
  }, [store]);

  const handleSaveFile = async (staged_file_id: string) => {
    setSaveStates((current) => {
      const next = new Map(current);
      next.set(staged_file_id, "saving");
      return next;
    });
    const destination = await pickSaveDestination();
    if (destination === null) {
      // Save dialog dismissed — no-op, retain staged result.
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
      // Save event from Rust will update state to "saved".
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
        saveStates.get(row.staged_file_id) !== "saved" &&
        saveStates.get(row.staged_file_id) !== "saving",
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
      // Save events from Rust will update individual states.
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

  const secondaryActions = (
    <>
      {isRunning && (
        <button
          className="action-button action-button--secondary"
          onClick={handleCancel}
        >
          <X size={16} strokeWidth={1.75} aria-hidden="true" />
          Cancel
        </button>
      )}
      {!isRunning && totals.total > 0 && (
        <button
          className="action-button action-button--secondary"
          onClick={handleReset}
        >
          <RefreshCw size={16} strokeWidth={1.75} aria-hidden="true" />
          Clear
        </button>
      )}
      {savableCount > 0 && !isRunning && (
        <Button
          size="sm"
          onClick={handleSaveAll}
          disabled={savableCount === 0}
        >
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
      dropLabel="Add images"
      formatHint="PNG · JPEG · WebP"
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
      onSave={handleSaveFile}
      options={
        <>
          <div className="workspace-section">
            <h3 className="workspace-section-title">Output format</h3>
            <Select
              label="Format"
              value={outputFormat}
              options={Object.entries(FORMAT_LABELS).map(([value, label]) => ({
                value: value as OutputFormat,
                label,
              }))}
              onChange={(value) => {
                setOutputFormat(value);
                if (value !== "jpeg") setQuality(85);
              }}
              hint="PNG and WebP are lossless; quality applies to JPEG only."
            />
          </div>
          {outputFormat === "jpeg" && (
            <div className="workspace-section">
              <h3 className="workspace-section-title">Quality</h3>
              <div className="option-row">
                <label className="option-label">JPEG quality</label>
                <input
                  type="range"
                  min="1"
                  max="100"
                  value={quality}
                  onChange={(e) => setQuality(Number(e.target.value))}
                  disabled={isRunning}
                />
                <span className="option-value">{quality}</span>
              </div>
              <span className="option-hint">
                Default 85. Transparent pixels are flattened onto white.
              </span>
            </div>
          )}
          <div className="workspace-section">
            <h3 className="workspace-section-title">Resize</h3>
            <div className="option-row">
              <label className="option-label">Resize images</label>
              <input
                type="checkbox"
                checked={resizeEnabled}
                onChange={(e) => setResizeEnabled(e.target.checked)}
                disabled={isRunning}
              />
              <span className="option-hint">
                When enabled, images are scaled to fit within the maximum
                width and height while preserving aspect ratio. Images are
                never stretched.
              </span>
            </div>
            {resizeEnabled && (
              <div className="option-row input-pair">
                <label className="option-label">Max width</label>
                <input
                  type="number"
                  min="1"
                  value={maxWidth}
                  onChange={(e) => setMaxWidth(e.target.value)}
                  placeholder="Width"
                  disabled={isRunning || !resizeEnabled}
                />
                <span>×</span>
                <label className="option-label">Max height</label>
                <input
                  type="number"
                  min="1"
                  value={maxHeight}
                  onChange={(e) => setMaxHeight(e.target.value)}
                  placeholder="Height"
                  disabled={isRunning || !resizeEnabled}
                />
              </div>
            )}
          </div>
        </>
      }
      actionLabel={convertLabel}
      onAction={handleConvert}
      actionDisabled={!canConvert}
      secondaryActions={secondaryActions}
    />
  );
}

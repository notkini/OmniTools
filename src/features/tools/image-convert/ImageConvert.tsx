import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { FolderOpen, RefreshCw, X } from "lucide-react";
import { openPath } from "@tauri-apps/plugin-opener";
import { Button } from "../../../components/ui/Button";
import { Select } from "../../../components/ui/Select";
import { ToolWorkspace } from "../../../components/ui/ToolWorkspace";
import { useToast } from "../../../components/feedback/Toast";
import { createQueueStore, type QueueStore } from "../../../shared/queue/store";
import {
  tauriInvokeAdapter,
  tauriListenAdapter,
  pickDirectory,
  pickImageFiles,
  pickImageDestination,
} from "../../../shared/fs/invoke";
import { parseToolError } from "../../../shared/fs/errors";

const CONVERT_COMMAND = "convert_files";
const CONVERT_CANCEL_COMMAND = "image_cancel";
const SAVE_COMMAND = "save_converted";
const SAVE_ALL_COMMAND = "save_all_converted";
const IMAGE_PROGRESS_EVENT = "omnitools://image-progress";
const IMAGE_SAVE_EVENT = "omnitools://image-save";
const THUMBNAIL_COMMAND = "image_thumbnail";
const DISCARD_COMMAND = "discard_staged";

type OutputFormat = "png" | "jpeg" | "webp";
type SaveState = "ready" | "saving" | "saved" | "saveError";

const FORMAT_LABELS: Record<OutputFormat, string> = {
  png: "PNG",
  jpeg: "JPEG",
  webp: "WebP",
};

/** Wire value (`ImageFormat` serde names) → display label. */
const WIRE_FORMAT_LABELS: Record<string, string> = {
  png: "PNG",
  jpeg: "JPEG",
  webp: "WebP",
};

/** Only the formats the bundled decoders genuinely support (Cargo `image`
 * features: png · jpeg · webp). Anything else is refused at add-time. */
const ACCEPTED_EXTENSIONS = new Set(["png", "jpg", "jpeg", "webp"]);

/** Mirrors `MAX_IMAGE_WIDTH/HEIGHT` in Rust (`image_limits`); clamped here
 * so the dialog can never submit a value the backend must reject. */
const MAX_DIMENSION_PX = 10_000;

let fallbackJobCounter = 0;

function makeImageJobId(): string {
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
    return crypto.randomUUID();
  }
  fallbackJobCounter += 1;
  return `image-job-${Date.now()}-${fallbackJobCounter}`;
}

/**
 * Single page-scoped store. Created once per app session — a store created
 * inside the render body would be re-created (and lose all rows) on every
 * render. The same singleton pattern File Hasher uses.
 */
let imageQueueStore: QueueStore | null = null;
function loadImageQueueStore(): QueueStore {
  if (imageQueueStore === null) {
    imageQueueStore = createQueueStore({
      invoke: tauriInvokeAdapter,
      listen: tauriListenAdapter,
      makeJobId: makeImageJobId,
      command: CONVERT_COMMAND,
      cancelCommand: CONVERT_CANCEL_COMMAND,
      progressEvent: IMAGE_PROGRESS_EVENT,
    });
  }
  return imageQueueStore;
}

/** Test hook: clears the module store so tests start from empty state. */
export function __resetImageQueueStoreForTests(): void {
  imageQueueStore = null;
}

function basenameOf(path: string): string {
  const parts = path.replace(/\\/g, "/").split("/");
  return parts[parts.length - 1] ?? path;
}

function dirnameOf(path: string): string {
  const normalized = path.replace(/\\/g, "/");
  const index = normalized.lastIndexOf("/");
  return index === -1 ? normalized : normalized.slice(0, index);
}

function extensionOf(path: string): string {
  const name = basenameOf(path);
  const dot = name.lastIndexOf(".");
  return dot === -1 ? "" : name.slice(dot + 1).toLowerCase();
}

/** Per user requirement: non-image input is explicitly not allowed. */
function splitImagePaths(paths: readonly string[]): {
  allowed: string[];
  rejected: string[];
} {
  const allowed: string[] = [];
  const rejected: string[] = [];
  for (const path of paths) {
    if (ACCEPTED_EXTENSIONS.has(extensionOf(path))) {
      allowed.push(path);
    } else {
      rejected.push(path);
    }
  }
  return { allowed, rejected };
}

function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)} MB`;
}

/** Narrows the `image-save` event payload honestly (unknown shapes ignored). */
function isSaveEvent(
  payload: unknown,
): payload is { stagedFileId: string; destination?: string; kind: "saved" | "saveError" } {
  if (typeof payload !== "object" || payload === null) return false;
  const candidate = payload as { stagedFileId?: unknown; kind?: unknown };
  return (
    typeof candidate.stagedFileId === "string" &&
    (candidate.kind === "saved" || candidate.kind === "saveError")
  );
}

export default function ImageConvertPage() {
  const toast = useToast();
  const store = useMemo(() => loadImageQueueStore(), []);
  const [outputFormat, setOutputFormat] = useState<OutputFormat>("jpeg");
  const [quality, setQuality] = useState(85);
  const [resizeEnabled, setResizeEnabled] = useState(false);
  const [maxWidth, setMaxWidth] = useState("");
  const [maxHeight, setMaxHeight] = useState("");

  const [snapshot, setSnapshot] = useState(() => store.getState());
  const [saveStates, setSaveStates] = useState<Map<string, SaveState>>(new Map());
  const [previews, setPreviews] = useState<Map<string, string>>(new Map());
  const [openDestination, setOpenDestination] = useState<string | null>(null);
  const thumbnailRequests = useRef(new Set<string>());

  useEffect(() => {
    return store.subscribe(() => setSnapshot(store.getState()));
  }, [store]);

  // Initialize save state to "ready" for newly completed conversions.
  useEffect(() => {
    setSaveStates((current) => {
      let changed = false;
      const next = new Map(current);
      for (const row of snapshot.rows) {
        if (
          row.status === "done" &&
          row.stagedFileId &&
          !next.has(row.stagedFileId)
        ) {
          next.set(row.stagedFileId, "ready");
          changed = true;
        }
      }
      return changed ? next : current;
    });
  }, [snapshot.rows]);

  // Fetch real thumbnails from Rust (decode+resize+encode all offline).
  useEffect(() => {
    for (const row of snapshot.rows) {
      const stagedFileId = row.stagedFileId;
      if (
        row.status !== "done" ||
        !stagedFileId ||
        thumbnailRequests.current.has(stagedFileId)
      ) {
        continue;
      }
      thumbnailRequests.current.add(stagedFileId);
      void tauriInvokeAdapter(THUMBNAIL_COMMAND, { stagedFileId })
        .then((result) => {
          if (typeof result !== "string") return;
          setPreviews((current) => {
            const next = new Map(current);
            next.set(stagedFileId, result);
            return next;
          });
        })
        .catch(() => {
          // No preview for this row — metadata and actions remain honest.
        });
    }
  }, [snapshot.rows]);

  // Listen for save completion events from Rust BEFORE any save is invoked.
  useEffect(() => {
    let active = true;
    let unlisten: (() => void) | undefined;
    void tauriListenAdapter(IMAGE_SAVE_EVENT, (payload: unknown) => {
      if (!isSaveEvent(payload)) return;
      const state = payload.kind === "saved" ? "saved" : "saveError";
      setSaveStates((current) => new Map(current).set(payload.stagedFileId, state));
      if (payload.kind === "saved" && typeof payload.destination === "string") {
        setOpenDestination(dirnameOf(payload.destination));
      }
    }).then((unlistenFn) => {
      if (active) {
        unlisten = unlistenFn;
      } else {
        unlistenFn();
      }
    });
    return () => {
      active = false;
      unlisten?.();
    };
  }, []);

  const { rows, phase } = snapshot;
  const isRunning = phase === "running";
  const hasFiles = rows.length > 0;
  const pendingCount = rows.filter((row) => row.status === "idle").length;
  const canConvert = hasFiles && pendingCount > 0 && !isRunning;

  const handleFiles = useCallback(
    (paths: string[]) => {
      const { allowed, rejected } = splitImagePaths(paths);
      if (rejected.length > 0) {
        const names = rejected.map(basenameOf);
        const shown = names.slice(0, 3).join(", ");
        const more = names.length > 3 ? ` and ${names.length - 3} more` : "";
        toast(
          `Not allowed: ${shown}${more} — only PNG, JPEG and WebP image files can be added.`,
          "error",
        );
      }
      if (allowed.length > 0) {
        store.addPaths(allowed);
      }
    },
    [store, toast],
  );

  const convertLabel = isRunning
    ? "Converting…"
    : pendingCount > 0
      ? `Convert ${pendingCount} file${pendingCount !== 1 ? "s" : ""}`
      : "Convert";

  const handleConvert = useCallback(async () => {
    if (!canConvert) {
      toast(
        isRunning ? "Conversion already in progress" : "No files queued for conversion",
        "error",
      );
      return;
    }
    let resize: { maxWidth: number; maxHeight: number } | undefined;
    if (resizeEnabled) {
      const width = Number.parseInt(maxWidth, 10);
      const height = Number.parseInt(maxHeight, 10);
      if (!(width >= 1 && height >= 1)) {
        toast("Set maximum width and height (whole numbers ≥ 1) to resize.", "error");
        return;
      }
      if (width > MAX_DIMENSION_PX || height > MAX_DIMENSION_PX) {
        toast(`Resize dimensions are limited to ${MAX_DIMENSION_PX} px.`, "error");
        return;
      }
      resize = { maxWidth: width, maxHeight: height };
    }
    try {
      // Wire contract: `ConvertOpts` is serde camelCase — outputFormat,
      // quality, resize.maxWidth / resize.maxHeight.
      await store.run({
        opts: {
          outputFormat,
          quality: outputFormat === "jpeg" ? quality : undefined,
          resize,
        },
      });
      const totals = store.getState().totals;
      const failed = totals.failed + totals.canceled;
      if (failed > 0) {
        toast(
          `Conversion finished with problems — ${failed} file${failed === 1 ? "" : "s"} need attention. See the queue for reasons.`,
          "error",
        );
      } else {
        toast(`Converted ${totals.done} file${totals.done === 1 ? "" : "s"} successfully.`, "success");
      }
    } catch (error) {
      const toolError = parseToolError(error);
      toast(`Conversion failed: ${toolError.message || "unknown error"}`, "error");
    }
  }, [
    canConvert,
    isRunning,
    resizeEnabled,
    maxWidth,
    maxHeight,
    outputFormat,
    quality,
    store,
    toast,
  ]);

  const handleCancel = useCallback(async () => {
    await store.cancel();
  }, [store]);

  const handleReset = useCallback(() => {
    const currentRows = store.getState().rows;
    const ids = currentRows
      .map((row) => row.stagedFileId)
      .filter((id): id is string => id !== undefined);
    store.reset();
    setSaveStates(new Map());
    setPreviews(new Map());
    thumbnailRequests.current.clear();
    setOpenDestination(null);
    if (ids.length > 0) {
      // Staged copies leave tmp storage now, not only on app exit.
      void tauriInvokeAdapter(DISCARD_COMMAND, { stagedFileIds: ids }).catch(() => {
        // Discard is best-effort housekeeping; TempDir purges on exit anyway.
      });
    }
  }, [store]);

  const handleRemovePath = useCallback(
    (path: string) => {
      const row = store.getState().rows.find((item) => item.path === path);
      store.removePath(path);
      if (row?.stagedFileId) {
        const stagedFileId = row.stagedFileId;
        thumbnailRequests.current.delete(stagedFileId);
        setPreviews((current) => {
          const next = new Map(current);
          next.delete(stagedFileId);
          return next;
        });
        setSaveStates((current) => {
          const next = new Map(current);
          next.delete(stagedFileId);
          return next;
        });
        void tauriInvokeAdapter(DISCARD_COMMAND, { stagedFileIds: [stagedFileId] }).catch(
          () => {
            // Best-effort; same as above.
          },
        );
      }
    },
    [store],
  );

  const handleSaveFile = useCallback(
    async (stagedFileId: string) => {
      const row = store
        .getState()
        .rows.find((item) => item.stagedFileId === stagedFileId);
      const suggestedName = row?.outputPath
        ? basenameOf(row.outputPath)
        : "converted.png";
      setSaveStates((current) => new Map(current).set(stagedFileId, "saving"));
      const destination = await pickImageDestination(suggestedName);
      if (destination === null) {
        // Save dialog dismissed — no-op, retain the staged result.
        setSaveStates((current) => new Map(current).set(stagedFileId, "ready"));
        return;
      }
      try {
        // Wire contract: `stagedFileId` binds to Rust's `staged_file_id`
        // (Tauri v2 camelCase-to-snake_case parameter matching).
        await tauriInvokeAdapter(SAVE_COMMAND, {
          stagedFileId,
          destination,
        });
        setSaveStates((current) => new Map(current).set(stagedFileId, "saved"));
        setOpenDestination(dirnameOf(destination));
      } catch (error) {
        const toolError = parseToolError(error);
        toast(`Save failed: ${toolError.message || "unknown error"}`, "error");
        setSaveStates((current) => new Map(current).set(stagedFileId, "saveError"));
      }
    },
    [store, toast],
  );

  const handleSaveAll = useCallback(async () => {
    const currentRows = store.getState().rows;
    const savable = currentRows.filter(
      (row) =>
        row.status === "done" &&
        row.stagedFileId &&
        saveStates.get(row.stagedFileId) !== "saved" &&
        saveStates.get(row.stagedFileId) !== "saving",
    );
    if (savable.length === 0) return;
    const destDir = await pickDirectory();
    if (destDir === null) return;
    const ids = savable.map((row) => row.stagedFileId!) as string[];
    setSaveStates((current) => {
      const next = new Map(current);
      for (const id of ids) next.set(id, "saving");
      return next;
    });
    try {
      // Wire contract: `dest_dir` / `staged_file_ids` bind from
      // `destDir` / `stagedFileIds` (camelCase-to-snake_case).
      const outcomes = (await tauriInvokeAdapter(SAVE_ALL_COMMAND, {
        destDir,
        stagedFileIds: ids,
      })) as ReadonlyArray<{
        kind: "saved" | "saveError";
        stagedFileId: string;
        destination?: string;
      }>;
      let savedCount = 0;
      let firstSavedDestination: string | null = null;
      for (const outcome of outcomes) {
        if (typeof outcome?.stagedFileId !== "string") continue;
        if (outcome.kind === "saved") {
          savedCount += 1;
          if (firstSavedDestination === null && typeof outcome.destination === "string") {
            firstSavedDestination = outcome.destination;
          }
        }
      }
      setSaveStates((current) => {
        const next = new Map(current);
        for (const outcome of outcomes) {
          if (typeof outcome?.stagedFileId !== "string") continue;
          next.set(
            outcome.stagedFileId,
            outcome.kind === "saved" ? "saved" : "saveError",
          );
        }
        return next;
      });
      if (savedCount > 0) {
        setOpenDestination(firstSavedDestination ? dirnameOf(firstSavedDestination) : destDir);
        toast(
          `Saved ${savedCount} file${savedCount === 1 ? "" : "s"} to ${dirnameOf(firstSavedDestination ?? destDir)}.`,
          "success",
        );
      } else {
        toast("Save all failed — no files were written. See row errors.", "error");
      }
    } catch (error) {
      const toolError = parseToolError(error);
      toast(`Save all failed: ${toolError.message || "unknown error"}`, "error");
      setSaveStates((current) => {
        const next = new Map(current);
        for (const id of ids) {
          if (next.get(id) === "saving") next.set(id, "saveError");
        }
        return next;
      });
    }
  }, [saveStates, store, toast]);

  const handleOpenLocation = useCallback(async () => {
    if (openDestination === null) return;
    try {
      await openPath(openDestination);
    } catch {
      toast("Could not open the output location.", "error");
    }
  }, [openDestination, toast]);

  const savableCount = rows.filter(
    (row) =>
      row.status === "done" &&
      row.stagedFileId &&
      saveStates.get(row.stagedFileId) === "ready",
  ).length;

  const secondaryActions = (
    <>
      {openDestination !== null && !isRunning ? (
        <button
          type="button"
          className="action-button action-button--secondary"
          onClick={() => void handleOpenLocation()}
        >
          <FolderOpen size={16} strokeWidth={1.75} aria-hidden="true" />
          Open output location
        </button>
      ) : null}
      {isRunning && (
        <button
          type="button"
          className="action-button action-button--secondary"
          onClick={handleCancel}
        >
          <X size={16} strokeWidth={1.75} aria-hidden="true" />
          Cancel
        </button>
      )}
      {!isRunning && snapshot.totals.total > 0 && (
        <button
          type="button"
          className="action-button action-button--secondary"
          onClick={handleReset}
        >
          <RefreshCw size={16} strokeWidth={1.75} aria-hidden="true" />
          Clear results
        </button>
      )}
      {savableCount > 0 && !isRunning && (
        <Button size="sm" onClick={handleSaveAll}>
          Save all ({savableCount})
        </Button>
      )}
    </>
  );

  return (
    <ToolWorkspace
      dropLabel="Add images"
      formatHint="PNG · JPEG · WebP"
      multiple
      onFiles={handleFiles}
      onBrowse={pickImageFiles}
      queue={rows.map((row) => ({
        id: row.id,
        path: row.path,
        name: row.name,
        status: row.status,
        size: row.sizeBytes !== undefined ? formatBytes(row.sizeBytes) : undefined,
        format: row.format
          ? (WIRE_FORMAT_LABELS[row.format] ?? row.format)
          : undefined,
        dimensions: row.dimensions,
        stagedFileId: row.stagedFileId,
        outputPath: row.outputPath,
        preview: row.stagedFileId ? previews.get(row.stagedFileId) : undefined,
        message: row.message,
        saveState: row.stagedFileId ? saveStates.get(row.stagedFileId) : undefined,
      }))}
      onSave={handleSaveFile}
      onRemove={handleRemovePath}
      options={
        <div className="tool-options">
          <Select
            label="Output format"
            value={outputFormat}
            options={Object.entries(FORMAT_LABELS).map(([value, label]) => ({
              value: value as OutputFormat,
              label,
            }))}
            onChange={(value) => {
              setOutputFormat(value);
              if (value !== "jpeg") setQuality(85);
            }}
            hint="PNG and WebP stay lossless."
          />
          {outputFormat === "jpeg" && (
            <div className="option-group">
              <div className="option-row">
                <label className="option-label" htmlFor="jpeg-quality">
                  Quality
                </label>
                <input
                  id="jpeg-quality"
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
                Transparency flattens onto white.
              </span>
            </div>
          )}
          <div className="option-group">
            <div className="option-row">
              <label className="option-label option-label--toggle" htmlFor="resize-toggle">
                Resize images
                <input
                  id="resize-toggle"
                  type="checkbox"
                  checked={resizeEnabled}
                  onChange={(e) => setResizeEnabled(e.target.checked)}
                  disabled={isRunning}
                />
              </label>
            </div>
            <span className="option-hint">
              Fits the maximum size, keeping aspect — never upscales.
            </span>
            {resizeEnabled && (
              <div className="option-row input-pair">
                <label className="option-label" htmlFor="resize-max-width">
                  Max width
                </label>
                <input
                  id="resize-max-width"
                  type="number"
                  min="1"
                  max={MAX_DIMENSION_PX}
                  value={maxWidth}
                  onChange={(e) => setMaxWidth(e.target.value)}
                  placeholder="Width"
                  disabled={isRunning || !resizeEnabled}
                />
                <span className="input-pair-x">×</span>
                <label className="option-label" htmlFor="resize-max-height">
                  Max height
                </label>
                <input
                  id="resize-max-height"
                  type="number"
                  min="1"
                  max={MAX_DIMENSION_PX}
                  value={maxHeight}
                  onChange={(e) => setMaxHeight(e.target.value)}
                  placeholder="Height"
                  disabled={isRunning || !resizeEnabled}
                />
              </div>
            )}
          </div>
        </div>
      }
      actionLabel={convertLabel}
      onAction={handleConvert}
      actionDisabled={!canConvert}
      secondaryActions={secondaryActions}
    />
  );
}

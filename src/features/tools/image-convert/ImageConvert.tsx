import { useCallback, useEffect, useState } from "react";
import { FolderOpen, RefreshCw, X } from "lucide-react";
import { createQueueStore } from "../../../shared/queue/store";
import {
  tauriInvokeAdapter,
  tauriListenAdapter,
  pickDirectory,
} from "../../../shared/fs/invoke";
import { ToolWorkspace } from "../../../components/ui/ToolWorkspace";
import { IconButton } from "../../../components/ui/IconButton";

const CONVERT_COMMAND = "convert_files";
const CONVERT_CANCEL_COMMAND = "image_cancel";
const IMAGE_PROGRESS_EVENT = "omnitools://image-progress";

type OutputFormat = "png" | "jpeg" | "webp";

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
  const [outputFormat, setOutputFormat] = useState<OutputFormat>("jpeg");
  const [quality, setQuality] = useState(85);
  const [maxWidth, setMaxWidth] = useState("");
  const [maxHeight, setMaxHeight] = useState("");
  const [outputDir, setOutputDir] = useState("");

  const store = createQueueStore({
    invoke: tauriInvokeAdapter,
    listen: tauriListenAdapter,
    makeJobId: () => crypto.randomUUID(),
    command: CONVERT_COMMAND,
    cancelCommand: CONVERT_CANCEL_COMMAND,
    progressEvent: IMAGE_PROGRESS_EVENT,
  });

  const [snapshot, setSnapshot] = useState(() => store.getState());

  useEffect(() => {
    return store.subscribe(() => setSnapshot(store.getState()));
  }, [store]);

  const { rows, phase, totals } = snapshot;
  const isRunning = phase === "running";
  const hasFiles = rows.length > 0;
  const allIdle = rows.every((row) => row.status === "idle");

  const handleFiles = useCallback(
    (paths: string[]) => {
      store.addPaths(paths);
    },
    [store],
  );

  const handleConvert = useCallback(async () => {
    if (!hasFiles || isRunning || !outputDir) return;
    const resize =
      maxWidth || maxHeight
        ? {
            max_width: maxWidth ? Number(maxWidth) : 0,
            max_height: maxHeight ? Number(maxHeight) : 0,
          }
        : undefined;
    await store.run({
      output_dir: outputDir,
      opts: {
        output_format: outputFormat,
        quality: outputFormat === "jpeg" ? quality : undefined,
        resize,
      },
    });
  }, [
    hasFiles,
    isRunning,
    outputDir,
    outputFormat,
    quality,
    maxWidth,
    maxHeight,
    store,
  ]);

  const handleCancel = useCallback(async () => {
    await store.cancel();
  }, [store]);

  const handleReset = useCallback(() => {
    store.reset();
  }, [store]);

  const handleOutputDir = async () => {
    const dir = await pickDirectory();
    if (dir) setOutputDir(dir);
  };

  const idleCount = rows.filter((row) => row.status === "idle").length;
  const canConvert = hasFiles && allIdle && !!outputDir && !isRunning;

  const convertLabel = isRunning
    ? "Converting…"
    : `Convert ${totals.total} file${totals.total !== 1 ? "s" : ""}`;

  const optionsPanel = (
    <section className="workspace-section">
      <h3 className="workspace-section-title">Output</h3>
      <div className="option-row">
        <label className="option-label">Format</label>
        <select
          value={outputFormat}
          onChange={(e) => setOutputFormat(e.target.value as OutputFormat)}
          disabled={isRunning}
        >
          {Object.entries(FORMAT_LABELS).map(([value, label]) => (
            <option key={value} value={value}>
              {label}
            </option>
          ))}
        </select>
      </div>

      {outputFormat === "jpeg" && (
        <div className="option-row">
          <label className="option-label">Quality</label>
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
      )}

      <div className="option-row">
        <label className="option-label">Resize (max)</label>
        <div className="input-pair">
          <input
            type="number"
            min="1"
            value={maxWidth}
            onChange={(e) => setMaxWidth(e.target.value)}
            placeholder="Width"
            disabled={isRunning}
          />
          <span>×</span>
          <input
            type="number"
            min="1"
            value={maxHeight}
            onChange={(e) => setMaxHeight(e.target.value)}
            placeholder="Height"
            disabled={isRunning}
          />
        </div>
        <span className="option-hint">
          Aspect ratio preserved — images are never stretched.
        </span>
      </div>

      <div className="option-row">
        <label className="option-label">Output folder</label>
        <div className="input-pair">
          <input
            type="text"
            value={outputDir}
            readOnly
            disabled={isRunning}
          />
          <IconButton
            label="Choose output folder"
            onClick={handleOutputDir}
            className="folder-button"
          >
            <FolderOpen size={16} strokeWidth={1.75} aria-hidden="true" />
          </IconButton>
        </div>
        {!outputDir && (
          <span className="option-hint">
            Choose where converted files will be saved.
          </span>
        )}
      </div>
    </section>
  );

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
        outputPath: row.outputPath,
        message: row.message,
      }))}
      options={optionsPanel}
      actionLabel={convertLabel}
      onAction={handleConvert}
      actionDisabled={!canConvert}
      secondaryActions={secondaryActions}
    />
  );
}

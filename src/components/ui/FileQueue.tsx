import { Copy, Download, FileQuestion, Save, X } from "lucide-react";
import { IconButton } from "./IconButton";

export type FileItemStatus = "idle" | "running" | "done" | "error" | "canceled";
export type SaveState = "ready" | "saving" | "saved" | "saveError";

export interface FileQueueItem {
  id: string;
  /** Absolute path as supplied by the registry queue (used for export). */
  path: string;
  name: string;
  status: FileItemStatus;
  /** Human-formatted size, e.g. "8.4 MB". Optional per row. */
  size?: string;
  /** Image format, e.g. "PNG", "JPEG", "WebP". Optional per row. */
  format?: string;
  /** Image dimensions, e.g. "1920 × 1080". Optional per row. */
  dimensions?: string;
  /** Lowercase hex SHA-256, shown once computed (File Hasher). */
  hex?: string;
  /** Opaque staged-file id for save operations (Image Converter). */
  stagedFileId?: string;
  /** Output path for converted files (Image Converter). */
  outputPath?: string;
  /** Rust-generated data-URL preview (Image Converter). */
  preview?: string;
  /** Per-file error message for status "error". */
  message?: string;
  /** Per-file save state (Image Converter). */
  saveState?: SaveState;
}

const STATUS_LABELS = {
  idle: "Queued",
  running: "Converting",
  done: "Done",
  canceled: "Canceled",
  error: "Error",
} as const;

const STATUS_CLASSES = {
  idle: "status-chip--idle",
  running: "status-chip--running",
  done: "status-chip--done",
  canceled: "status-chip--canceled",
  error: "status-chip--error",
} as const;

interface FileQueueProps {
  items: FileQueueItem[];
  onCopyHex?: (hex: string) => void;
  onCopyPath?: (path: string) => void;
  onSave?: (stagedFileId: string) => void;
  onRemove?: (path: string) => void;
}

/**
 * Honest per-file status list: one row per file, real state, real messages.
 * A failed or canceled file never stops the batch — it is reported here
 * (PLAN §4.4 / M4 correction 1).
 */
export function FileQueue({ items, onCopyHex, onCopyPath, onSave, onRemove }: FileQueueProps) {
  return (
    <ul className="file-queue" aria-label="File queue">
      {items.map((item) => (
        <li
          key={item.id}
          className="file-queue-row"
          title={item.message ?? item.path}
          data-path={item.path}
        >
          {item.preview ? (
            <img
              className="file-preview"
              src={item.preview}
              alt=""
              loading="lazy"
              aria-hidden="true"
            />
          ) : (
            <FileQuestion size={16} strokeWidth={1.5} aria-hidden="true" />
          )}
          <span className="file-queue-cell">
            <span className="file-name">{item.name}</span>
            {item.format && item.dimensions ? (
              <span className="file-meta">{item.format} · {item.dimensions}</span>
            ) : null}
            {item.message ? (
              <span className="file-error" role="alert">
                {item.message}
              </span>
            ) : null}
          </span>
          {item.size ? <span className="file-size">{item.size}</span> : null}
          {item.hex && onCopyHex ? (
            <IconButton
              label={`Copy SHA-256 of ${item.name}`}
              className="row-copy-button"
              onClick={() => void onCopyHex(item.hex!)}
            >
              <Copy size={14} strokeWidth={1.75} aria-hidden="true" />
            </IconButton>
          ) : null}
          {item.outputPath && onCopyPath ? (
            <IconButton
              label={`Copy output path of ${item.name}`}
              className="row-copy-button"
              onClick={() => void onCopyPath(item.outputPath!)}
            >
              <Copy size={14} strokeWidth={1.75} aria-hidden="true" />
            </IconButton>
          ) : null}
          {item.stagedFileId && onSave ? (
            <IconButton
              label={
                item.saveState === "saved"
                  ? `${item.name} saved`
                  : `Save ${item.name}…`
              }
              className="row-save-button"
              disabled={item.saveState === "saving" || item.saveState === "saved"}
              onClick={() => void onSave(item.stagedFileId!)}
            >
              {item.saveState === "saved" ? (
                <Download size={14} strokeWidth={1.75} aria-hidden="true" />
              ) : (
                <Save size={14} strokeWidth={1.75} aria-hidden="true" />
              )}
            </IconButton>
          ) : null}
          {onRemove ? (
            <IconButton
              label={`Remove ${item.name} from queue`}
              className="row-remove-button"
              disabled={item.status === "running"}
              onClick={() => void onRemove(item.path)}
            >
              <X size={14} strokeWidth={1.75} aria-hidden="true" />
            </IconButton>
          ) : null}
          <span className={`status-chip ${STATUS_CLASSES[item.status]}`}>
            {STATUS_LABELS[item.status]}
            {item.saveState === "saved" ? " · Saved" : null}
            {item.saveState === "saveError" ? " · Save failed" : null}
          </span>
        </li>
      ))}
    </ul>
  );
}

import { FileQuestion } from "lucide-react";

export type FileItemStatus = "idle" | "running" | "done" | "error";

export interface FileQueueItem {
  id: string;
  name: string;
  status: FileItemStatus;
  /** Human-formatted sizes, e.g. "8.4 MB → 3.1 MB". Optional per row. */
  size?: string;
  /** Per-file error message for status "error". */
  message?: string;
}

const STATUS_LABELS = {
  idle: "Queued",
  running: "Processing",
  done: "Done",
  error: "Error",
} as const;

const STATUS_CLASSES = {
  idle: "status-chip--idle",
  running: "status-chip--running",
  done: "status-chip--done",
  error: "status-chip--error",
} as const;

/**
 * Honest per-file status list: one row per file, real state, real messages.
 * A failed file never stops the batch — it is reported here (PLAN §4.4).
 */
export function FileQueue({ items }: { items: FileQueueItem[] }) {
  return (
    <ul className="file-queue" aria-label="File queue">
      {items.map((item) => (
        <li
          key={item.id}
          className="file-queue-row"
          title={item.message ?? item.name}
        >
          <FileQuestion size={16} strokeWidth={1.5} aria-hidden="true" />
          <span className="file-name">{item.name}</span>
          {item.size ? <span className="file-size">{item.size}</span> : null}
          <span className={`status-chip ${STATUS_CLASSES[item.status]}`}>
            {STATUS_LABELS[item.status]}
          </span>
        </li>
      ))}
    </ul>
  );
}

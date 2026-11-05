import type { ReactNode } from "react";
import { FileDropzone } from "./FileDropzone";
import { FileQueue, type FileQueueItem } from "./FileQueue";

/** Shape of one live-preview row, shared by tools that use the preview region. */
export interface PreviewRow {
  original: string;
  proposed: string;
  status: "ok" | "conflict" | "unchanged";
  reason: string;
  skipped: boolean;
}

interface ToolWorkspaceProps {
  /** Dropzone copy, e.g. "Drop files or click to browse". */
  dropLabel: string;
  /** Accepted formats readout shown beneath the drop label (§4.2). */
  formatHint?: string;
  multiple?: boolean;
  /** Receives absolute filesystem paths — processing happens in Rust. */
  onFiles: (paths: string[]) => void;
  /** Rows are owned by the tool's state; may be empty. */
  queue: FileQueueItem[];
  onCopyHex?: (hex: string) => void;
  onCopyPath?: (path: string) => void;
  /** Forwarded to save buttons on queue rows (Image Converter). */
  onSave?: (stagedFileId: string) => void;
  /** Forwarded to remove buttons on queue rows (Image Converter). */
  onRemove?: (path: string) => void;
  /** Forwarded to the dropzone's click-to-browse (injectable for tests). */
  onBrowse?: () => Promise<string[] | null>;
  /** Options panel content (real options only — no placeholder controls). */
  options?: ReactNode;
  /** e.g. "Hash 12 files" or null while nothing actionable. */
  actionLabel?: string;
  onAction?: () => void;
  actionDisabled?: boolean;
  /** Secondary actions (Cancel / Export) rendered beside the primary. */
  secondaryActions?: ReactNode;
  /**
   * Optional live preview region. Only tools that define it render this panel;
   * existing tools continue to work with `undefined` here (backward-compatible).
   */
  preview?: ReactNode;
  /** Used as the `aria-label` for the preview table when provided. */
  previewAriaLabel?: string;
}

/**
 * THE reusable tool page template (PLAN §4.2). Every tool renders inside
 * these regions: dropzone → queue → options → action bar. Tools must not
 * invent their own layout.
 *
 * The dropzone stays available while the queue is non-empty (compact
 * variant) so users can always append more files to a finished batch.
 */
export function ToolWorkspace({
  dropLabel,
  formatHint,
  multiple = false,
  onFiles,
  queue,
  onCopyHex,
  onCopyPath,
  onSave,
  onRemove,
  onBrowse,
  options,
  actionLabel,
  onAction,
  actionDisabled = false,
  secondaryActions,
  preview,
  previewAriaLabel,
}: ToolWorkspaceProps) {
  const hasQueue = queue.length > 0;
  return (
    <div className="tool-workspace">
      {hasQueue ? <FileQueue items={queue} onCopyHex={onCopyHex} onCopyPath={onCopyPath} onSave={onSave} onRemove={onRemove} /> : null}
      {preview ? (
        <section className="workspace-preview-region" aria-label={previewAriaLabel}>
          {preview}
        </section>
      ) : null}
      <FileDropzone
        label={hasQueue ? "Add more files" : dropLabel}
        formatHint={formatHint}
        multiple={multiple}
        compact={hasQueue}
        onFiles={onFiles}
        onBrowse={onBrowse}
      />
      {options}
      {actionLabel && onAction ? (
        <div className="action-bar">
          {secondaryActions}
          <button
            type="button"
            className="btn btn--primary"
            disabled={actionDisabled}
            onClick={onAction}
          >
            {actionLabel}
          </button>
        </div>
      ) : secondaryActions ? (
        <div className="action-bar">{secondaryActions}</div>
      ) : null}
    </div>
  );
}

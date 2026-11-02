import type { ReactNode } from "react";
import { FileDropzone } from "./FileDropzone";
import { FileQueue, type FileQueueItem } from "./FileQueue";

interface ToolWorkspaceProps {
  /** Dropzone copy, e.g. "Drop files or click to browse". */
  dropLabel: string;
  /** Accepted formats readout shown beneath the drop label (§4.2). */
  formatHint?: string;
  multiple?: boolean;
  /** Receives absolute filesystem paths — processing happens in Rust. */
  onFiles: (paths: string[]) => void;
  /** Rows are owned by the tool's state — empty means show the dropzone. */
  queue: FileQueueItem[];
  onCopyHex?: (hex: string) => void;
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
}

/**
 * THE reusable tool page template (PLAN §4.2). Every tool renders inside
 * these regions: dropzone → queue → options → action bar. Tools must not
 * invent their own layout.
 */
export function ToolWorkspace({
  dropLabel,
  formatHint,
  multiple = false,
  onFiles,
  queue,
  onCopyHex,
  onBrowse,
  options,
  actionLabel,
  onAction,
  actionDisabled = false,
  secondaryActions,
}: ToolWorkspaceProps) {
  return (
    <div className="tool-workspace">
      {queue.length === 0 ? (
        <FileDropzone
          label={dropLabel}
          formatHint={formatHint}
          multiple={multiple}
          onFiles={onFiles}
          onBrowse={onBrowse}
        />
      ) : (
        <FileQueue items={queue} onCopyHex={onCopyHex} />
      )}
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

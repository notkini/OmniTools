import type { ReactNode } from "react";
import { FileDropzone } from "./FileDropzone";
import { FileQueue, type FileQueueItem } from "./FileQueue";

interface ToolWorkspaceProps {
  /** Dropzone copy, e.g. "Drop images or click to browse". */
  dropLabel: string;
  /** Accepted formats readout shown beneath the drop label (§4.2). */
  formatHint?: string;
  multiple?: boolean;
  onFiles: (files: File[]) => void;
  /** Rows are owned by the tool's state — empty means show the dropzone. */
  queue: FileQueueItem[];
  /** Options panel content (real options only — no placeholder controls). */
  options?: ReactNode;
  /** e.g. "Convert 12 files →" or null while nothing actionable. */
  actionLabel?: string;
  onAction?: () => void;
  actionDisabled?: boolean;
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
  options,
  actionLabel,
  onAction,
  actionDisabled = false,
}: ToolWorkspaceProps) {
  return (
    <div className="tool-workspace">
      {queue.length === 0 ? (
        <FileDropzone
          label={dropLabel}
          formatHint={formatHint}
          multiple={multiple}
          onFiles={onFiles}
        />
      ) : (
        <FileQueue items={queue} />
      )}
      {options}
      {actionLabel && onAction ? (
        <div className="action-bar">
          <button
            type="button"
            className="btn btn--primary"
            disabled={actionDisabled}
            onClick={onAction}
          >
            {actionLabel}
          </button>
        </div>
      ) : null}
    </div>
  );
}

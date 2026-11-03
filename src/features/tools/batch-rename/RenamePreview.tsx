import { useMemo } from "react";

/**
 * Per-row preview for the batch rename tool. Renders one row per file with
 * its original name, proposed name, status chip, reason, and skip toggle.
 *
 * RenameProposal mirrors the Rust type defined in
 * src-tauri/src/processing/renaming.rs and is serialized across IPC.
 */
export interface RenameProposal {
  fromName: string;
  toName: string;
  kind: "ok" | "conflict" | "unchanged";
  reason: string;
}

interface RenamePreviewProps {
  items: RenameProposal[];
  skipPaths: Set<string>;
  onSkipToggle: (path: string) => void;
  disabled?: boolean;
}

const STATUS_LABELS = {
  ok: "OK",
  conflict: "Conflict",
  unchanged: "Unchanged",
} as const;

const STATUS_CLASSES = {
  ok: "status-chip--ok",
  conflict: "status-chip--conflict",
  unchanged: "status-chip--unchanged",
} as const;

export function RenamePreview({
  items,
  skipPaths,
  onSkipToggle,
  disabled = false,
}: RenamePreviewProps) {
  const conflictCount = useMemo(
    () => items.filter((i) => i.kind === "conflict").length,
    [items],
  );
  const unchangedCount = useMemo(
    () => items.filter((i) => i.kind === "unchanged").length,
    [items],
  );

  if (items.length === 0) {
    return (
      <div className="preview-panel">
        <div className="preview-empty">
          <span className="preview-empty-label">
            No files selected — drop files into the dropzone above
          </span>
        </div>
      </div>
    );
  }

  return (
    <div className="preview-panel">
      <div className="preview-header">
        <span className="preview-title">
          Preview ({items.length} file{items.length !== 1 ? "s" : ""})
        </span>
        {conflictCount > 0 && (
          <span className="preview-conflict-count">
            {conflictCount} conflict{conflictCount !== 1 ? "s" : ""}
          </span>
        )}
        {unchangedCount > 0 && (
          <span className="preview-unchanged-count">
            {unchangedCount} unchanged
          </span>
        )}
      </div>
      <table className="preview-table" aria-label="Rename preview">
        <thead>
          <tr>
            <th className="preview-col-select" scope="col">
              <span className="visually-hidden">Skip</span>
            </th>
            <th className="preview-col-original" scope="col">
              Original file
            </th>
            <th className="preview-col-proposed" scope="col">
              Proposed name
            </th>
            <th className="preview-col-status" scope="col">
              Status
            </th>
            <th className="preview-col-reason" scope="col">
              Reason
            </th>
          </tr>
        </thead>
        <tbody>
          {items.map((item) => (
            <tr
              key={item.fromName}
              className={`preview-row preview-row--${item.kind}`}
              data-testid={`preview-row-${item.fromName}`}
            >
              <td className="preview-cell-select">
                <input
                  type="checkbox"
                  checked={skipPaths.has(item.fromName)}
                  disabled={disabled}
                  onChange={() => void onSkipToggle(item.fromName)}
                  aria-label={`Skip ${item.fromName}`}
                />
              </td>
              <td className="preview-cell-original">
                <span className="preview-file-name">{item.fromName}</span>
              </td>
              <td className="preview-cell-proposed">
                <span
                  className={`preview-proposed-name ${
                    item.kind === "conflict"
                      ? "preview-proposed-name--conflict"
                      : ""
                  }`}
                >
                  {item.toName}
                </span>
              </td>
              <td className="preview-cell-status">
                <span className={`status-chip ${STATUS_CLASSES[item.kind]}`}>
                  {STATUS_LABELS[item.kind]}
                </span>
              </td>
              <td className="preview-cell-reason">
                {item.reason ? (
                  <span className="preview-reason" title={item.reason}>
                    {item.reason}
                  </span>
                ) : (
                  <span className="preview-reason--empty">—</span>
                )}
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

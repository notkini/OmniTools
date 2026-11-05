import { useMemo, type ReactNode } from "react";

/**
 * Per-row preview for the batch rename tool. Renders one row per file with
 * its original name, proposed name, status chip, reason, and skip toggle.
 *
 * `RenameProposalRow` mirrors the Rust `RenameProposal` type defined in
 * src-tauri/src/processing/renaming.rs, serialized camelCase across IPC.
 */
export interface RenameProposalRow {
  fromName: string;
  toName: string;
  kind: "ok" | "conflict" | "unchanged";
  reason: string;
}

interface RenamePreviewProps {
  /** Proposals in input order — aligned 1:1 with `paths`. */
  items: RenameProposalRow[];
  /** Absolute paths in the same input order. */
  paths: string[];
  skipPaths: Set<string>;
  onSkipToggle: (path: string) => void;
  disabled?: boolean;
  /** A plan request is in flight; results will replace these rows. */
  loading?: boolean;
  /** Failed plan requests show this honest error instead of stale rows. */
  error?: string | null;
}

const STATUS_LABELS: Record<RenameProposalRow["kind"], string> = {
  ok: "OK",
  conflict: "Conflict",
  unchanged: "Unchanged",
};

const STATUS_CLASSES: Record<RenameProposalRow["kind"], string> = {
  ok: "status-chip--ok",
  conflict: "status-chip--conflict",
  unchanged: "status-chip--unchanged",
};

export function RenamePreview({
  items,
  paths,
  skipPaths,
  onSkipToggle,
  disabled = false,
  loading = false,
  error = null,
}: RenamePreviewProps) {
  const conflictCount = useMemo(
    () => items.filter((item) => item.kind === "conflict").length,
    [items],
  );
  const unchangedCount = useMemo(
    () => items.filter((item) => item.kind === "unchanged").length,
    [items],
  );

  const header = (
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
      {loading && (
        <span className="preview-loading-label" role="status">
          Computing preview…
        </span>
      )}
    </div>
  );

  let body: ReactNode;
  if (error) {
    body = (
      <div className="preview-empty">
        <span className="preview-empty-label">Preview failed: {error}</span>
      </div>
    );
  } else if (items.length === 0) {
    body = (
      <div className="preview-empty">
        <span className="preview-empty-label">
          No files selected — drop files into the dropzone to begin
        </span>
      </div>
    );
  } else {
    body = (
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
          {items.map((item, index) => {
            const path = paths[index] ?? item.fromName;
            return (
              <tr
                key={path}
                className={`preview-row preview-row--${item.kind}`}
                data-testid={`preview-row-${index + 1}`}
              >
                <td className="preview-cell-select">
                  <input
                    type="checkbox"
                    checked={skipPaths.has(path)}
                    disabled={disabled}
                    onChange={() => onSkipToggle(path)}
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
            );
          })}
        </tbody>
      </table>
    );
  }

  return (
    <div className="preview-panel">
      {header}
      {body}
    </div>
  );
}

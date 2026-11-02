import {
  useCallback,
  useEffect,
  useMemo,
  useState,
  useSyncExternalStore,
} from "react";
import { Button } from "../../../components/ui/Button";
import { type FileQueueItem } from "../../../components/ui/FileQueue";
import { Select, TextInput } from "../../../components/ui/Select";
import { ToolWorkspace } from "../../../components/ui/ToolWorkspace";
import { useToast } from "../../../components/feedback/Toast";
import {
  describeAppErrorCode,
  parseToolError,
} from "../../../shared/fs/errors";
import {
  pickFiles,
  pickSaveDestination,
  tauriInvokeAdapter,
  tauriListenAdapter,
} from "../../../shared/fs/invoke";
import {
  createQueueStore,
  type QueueRow,
  type QueueStore,
} from "../../../shared/queue/store";

/** Injectable side-effect surface — overridden in tests. */
export interface HashToolDeps {
  pickFiles: () => Promise<string[] | null>;
  pickSaveDestination: () => Promise<string | null>;
  exportChecksums: (destination: string, entries: readonly string[]) => Promise<void>;
  compareFiles: (first: string, second: string) => Promise<{ identical: boolean }>;
}

const defaultDeps: HashToolDeps = {
  pickFiles,
  pickSaveDestination,
  exportChecksums: async (destination, entries) => {
    await tauriInvokeAdapter("export_checksums", { destination, entries });
  },
  compareFiles: async (first, second) => {
    const outcome = (await tauriInvokeAdapter("compare_files", {
      first,
      second,
    })) as { identical: boolean };
    return outcome;
  },
};

let fallbackJobCounter = 0;

/** Fresh id per batch — required explicitly by the queue store contract. */
export function makeJobId(): string {
  if (
    typeof crypto !== "undefined" &&
    typeof crypto.randomUUID === "function"
  ) {
    try {
      return crypto.randomUUID();
    } catch {
      // Fall through to the deterministic fallback.
    }
  }
  fallbackJobCounter += 1;
  return `job-${Date.now()}-${fallbackJobCounter}`;
}

/** Process-wide singleton so the queue survives page revisits. */
let sharedStore: QueueStore | null = null;

function defaultStore(): QueueStore {
  sharedStore ??= createQueueStore({
    invoke: tauriInvokeAdapter,
    listen: tauriListenAdapter,
    makeJobId,
  });
  return sharedStore;
}

/** Human size from raw bytes — B → KB → MB → GB → TB. */
export function formatSize(bytes: number | undefined): string | undefined {
  if (bytes === undefined || Number.isNaN(bytes)) return undefined;
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB", "TB"] as const;
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value < 10 ? value.toFixed(1) : Math.round(value)} ${units[unit]}`;
}

const HEX_RE = /^[0-9a-f]{64}$/;

/** Normalises a pasted digest: lowercase, whitespace and colons removed. */
export function normalizeDigest(input: string): string {
  return input.toLowerCase().replace(/[\s:]/g, "");
}

function toQueueItems(rows: readonly QueueRow[]): FileQueueItem[] {
  return rows.map((row) => ({
    id: row.id,
    path: row.path,
    name: row.name,
    status: row.status,
    size: formatSize(row.sizeBytes),
    hex: row.hex,
    message: row.message,
  }));
}

interface HashToolPageProps {
  store?: QueueStore;
  deps?: Partial<HashToolDeps>;
}

/** File Hasher — SHA-256 batch hashing, compare, and verify (Milestone 4). */
export default function HashToolPage({ store, deps }: HashToolPageProps) {
  const resolvedDeps = useMemo<HashToolDeps>(
    () => ({ ...defaultDeps, ...deps }),
    [deps],
  );
  const queueStore = useMemo(() => store ?? defaultStore(), [store]);

  const state = useSyncExternalStore(
    queueStore.subscribe,
    queueStore.getState,
  );
  const toast = useToast();

  const rows = state.rows;
  const running = state.phase === "running";
  const idleCount = rows.filter((row) => row.status === "idle").length;
  const exportableRows = rows
    .filter((row) => row.status === "done" && typeof row.hex === "string")
    .filter((row) => row.hex !== undefined && row.hex.length > 0);

  const [exporting, setExporting] = useState(false);

  const [compareFirst, setCompareFirst] = useState("");
  const [compareSecond, setCompareSecond] = useState("");
  const [comparing, setComparing] = useState(false);
  const [compareResult, setCompareResult] = useState<"identical" | "different" | null>(null);
  const [compareError, setCompareError] = useState<string | null>(null);

  const [verifyPath, setVerifyPath] = useState("");
  const [verifyInput, setVerifyInput] = useState("");
  const [verifyResult, setVerifyResult] = useState<"match" | "mismatch" | null>(null);

  const normalizedVerify = normalizeDigest(verifyInput);

  // Cleared selections when the queue empties — stale paths must not linger.
  const hasRows = rows.length > 0;
  useEffect(() => {
    if (hasRows) return;
    setCompareFirst("");
    setCompareSecond("");
    setCompareResult(null);
    setCompareError(null);
    setVerifyPath("");
    setVerifyInput("");
    setVerifyResult(null);
  }, [hasRows]);

  const addFiles = useCallback(async () => {
    const paths = await resolvedDeps.pickFiles();
    if (paths && paths.length > 0) {
      queueStore.addPaths(paths);
    }
  }, [resolvedDeps, queueStore]);

  const copyHex = useCallback(async (hex: string) => {
    if (!hex) return;
    try {
      if (!navigator.clipboard) throw new Error("clipboard unavailable");
      await navigator.clipboard.writeText(hex);
      toast("SHA-256 copied to clipboard", "success");
    } catch {
      toast("Could not access the clipboard", "error");
    }
  }, [toast]);

  const exportChecksums = useCallback(async () => {
    const entries = exportableRows.map(
      (row) => `${row.hex}  *${row.path}`,
    );
    if (entries.length === 0) return;
    const destination = await resolvedDeps.pickSaveDestination();
    // Dismissing the save dialog is a no-op — nothing is written.
    if (destination === null) return;
    setExporting(true);
    try {
      await resolvedDeps.exportChecksums(destination, entries);
      toast(
        `Exported ${entries.length} checksum${entries.length === 1 ? "" : "s"} to ${destination}`,
        "success",
      );
    } catch (error) {
      const toolError = parseToolError(error);
      const detail = toolError.message || describeAppErrorCode(toolError.code);
      toast(`Export failed: ${detail}`, "error");
    } finally {
      setExporting(false);
    }
  }, [exportableRows, resolvedDeps, toast]);

  const runCompare = useCallback(async () => {
    if (!compareFirst || !compareSecond) return;
    setComparing(true);
    setCompareResult(null);
    setCompareError(null);
    try {
      const outcome = await resolvedDeps.compareFiles(compareFirst, compareSecond);
      setCompareResult(outcome.identical ? "identical" : "different");
    } catch (error) {
      const toolError = parseToolError(error);
      const detail = toolError.message || describeAppErrorCode(toolError.code);
      setCompareError(detail);
    } finally {
      setComparing(false);
    }
  }, [compareFirst, compareSecond, resolvedDeps]);

  const verifyDigest = useCallback(() => {
    const row = rows.find((candidate) => candidate.path === verifyPath);
    if (!row || typeof row.hex !== "string" || !HEX_RE.test(normalizedVerify)) {
      return;
    }
    setVerifyResult(
      normalizedVerify === row.hex.toLowerCase() ? "match" : "mismatch",
    );
  }, [rows, verifyPath, normalizedVerify]);

  const fileOptions = useMemo(
    () => rows.map((row) => ({ value: row.path, label: row.name })),
    [rows],
  );

  // Primary action mirrors the live queue: cancel while running, hash what
  // is queued, otherwise offer the export of what completed.
  let actionLabel: string;
  let onAction: () => void;
  let actionDisabled = false;
  if (running) {
    actionLabel = "Cancel";
    onAction = () => void queueStore.cancel();
  } else if (idleCount > 0) {
    actionLabel = `Hash ${idleCount} file${idleCount === 1 ? "" : "s"}…`;
    onAction = () => void queueStore.run();
  } else {
    actionLabel = "Export checksums…";
    onAction = () => void exportChecksums();
    actionDisabled = exporting || exportableRows.length === 0;
  }

  const secondaryActions =
    running ? null : (
      <>
        <Button size="sm" onClick={() => void addFiles()}>
          Add files
        </Button>
        {idleCount === 0 && exportableRows.length > 0 && actionLabel !== "Export checksums…" && (
          <Button
            size="sm"
            disabled={exporting}
            onClick={() => void exportChecksums()}
          >
            Export checksums…
          </Button>
        )}
        <Button size="sm" onClick={() => queueStore.reset()}>
          Clear
        </Button>
      </>
    );

  return (
    <div className="view-content">
      <h1 className="page-title">File Hasher</h1>
      <ToolWorkspace
        dropLabel="Drop files or click to browse"
        formatHint="SHA-256 · processed locally, nothing leaves your machine"
        multiple
        onFiles={(paths) => queueStore.addPaths(paths)}
        onBrowse={resolvedDeps.pickFiles}
        queue={toQueueItems(rows)}
        onCopyHex={(hex) => void copyHex(hex)}
        actionLabel={rows.length === 0 ? undefined : actionLabel}
        onAction={rows.length === 0 ? undefined : onAction}
        actionDisabled={actionDisabled}
        secondaryActions={secondaryActions}
        options={
          hasRows ? (
            <div className="hash-panels">
              <section className="hash-panel" aria-label="Compare two files">
                <h2 className="section-title">Compare two files</h2>
                <div className="hash-panel-fields">
                  <Select
                    label="First file"
                    value={compareFirst}
                    options={[{ value: "", label: "Select a file…" }, ...fileOptions]}
                    onChange={(value) => {
                      setCompareFirst(value);
                      setCompareResult(null);
                      setCompareError(null);
                    }}
                  />
                  <Select
                    label="Second file"
                    value={compareSecond}
                    options={[{ value: "", label: "Select a file…" }, ...fileOptions]}
                    onChange={(value) => {
                      setCompareSecond(value);
                      setCompareResult(null);
                      setCompareError(null);
                    }}
                  />
                  <div className="hash-panel-action">
                    <Button
                      size="sm"
                      disabled={
                        !compareFirst ||
                        !compareSecond ||
                        comparing ||
                        fileOptions.length < 2
                      }
                      onClick={() => void runCompare()}
                    >
                      {comparing ? "Comparing…" : "Compare"}
                    </Button>
                  </div>
                </div>
                {compareResult === "identical" && (
                  <p className="result-banner result-banner--success" role="status">
                    Identical — the files match byte for byte.
                  </p>
                )}
                {compareResult === "different" && (
                  <p className="result-banner result-banner--danger" role="status">
                    Different — the files do not match.
                  </p>
                )}
                {compareError !== null && (
                  <p className="result-banner result-banner--danger" role="alert">
                    Compare failed: {compareError}
                  </p>
                )}
              </section>

              <section className="hash-panel" aria-label="Verify a digest">
                <h2 className="section-title">Verify a digest</h2>
                <div className="hash-panel-fields">
                  <Select
                    label="File"
                    value={verifyPath}
                    options={[{ value: "", label: "Select a file…" }, ...fileOptions]}
                    onChange={(value) => {
                      setVerifyPath(value);
                      setVerifyResult(null);
                    }}
                  />
                  <div className="hash-panel-verify-field">
                    <TextInput
                      label="Expected SHA-256"
                      value={verifyInput}
                      placeholder="Paste a digest (spaces or colons are fine)"
                      onChange={(value) => {
                        setVerifyInput(value);
                        setVerifyResult(null);
                      }}
                    />
                  </div>
                  <div className="hash-panel-action">
                    <Button
                      size="sm"
                      disabled={!verifyPath || !HEX_RE.test(normalizedVerify)}
                      onClick={verifyDigest}
                    >
                      Verify
                    </Button>
                  </div>
                </div>
                {verifyResult === "match" && (
                  <p className="result-banner result-banner--success" role="status">
                    Match — the digest matches the file.
                  </p>
                )}
                {verifyResult === "mismatch" && (
                  <p className="result-banner result-banner--danger" role="alert">
                    No match — this is not the SHA-256 of the selected file.
                  </p>
                )}
              </section>
            </div>
          ) : null
        }
      />
    </div>
  );
}

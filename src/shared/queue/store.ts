/**
 * Hand-rolled, dependency-free batch queue store (PLAN M4 task 3).
 * Adapters (invoke/event) are injectable so tests drive the full state
 * machine without launching Tauri. Never re-exposed as a framework store —
 * consumers subscribe via `useSyncExternalStore` or directly.
 */
import { describeAppErrorCode, parseToolError, type AppErrorCode } from "../fs/errors";

/** Single canonical command + event channel (approved inconsistency fix). */
export const HASH_COMMAND = "hash_files";
export const HASH_CANCEL_COMMAND = "hash_cancel";
export const HASH_PROGRESS_EVENT = "omnitools://hash-progress";

export type QueueStatus = "idle" | "running" | "done" | "error" | "canceled";
export type JobPhase = "idle" | "running" | "finished";

export interface QueueRow {
  id: string;
  path: string;
  name: string;
  status: QueueStatus;
  hex?: string;
  sizeBytes?: number;
  /** Output path for converted files (Image Converter). */
  outputPath?: string;
  message?: string;
}

export interface QueueTotals {
  total: number;
  running: number;
  done: number;
  failed: number;
  canceled: number;
}

export interface QueueSnapshot {
  rows: readonly QueueRow[];
  phase: JobPhase;
  totals: QueueTotals;
}

/** Wire shape of `hash_files`' authoritative return value. */
export type HashOutcome =
  | { kind: "completed"; path: string; hex: string; sizeBytes: number }
  | { kind: "failed"; path: string; error: { code: AppErrorCode; message: string } }
  | { kind: "canceled"; path: string };

/** Wire shape of `omnitools://hash-progress` payloads. */
export interface HashProgressPayload {
  jobId: string;
  path: string;
  phase: "started" | "completed" | "error" | "canceled";
  hex?: string;
  sizeBytes?: number;
  code?: string;
  message?: string;
}

/** Wire shape of `omnitools://image-progress` payloads. */
export interface ImageProgressPayload {
  jobId: string;
  path: string;
  phase: "started" | "completed" | "error" | "canceled";
  inputSizeBytes?: number;
  outputSizeBytes?: number;
  outputPath?: string;
  code?: string;
  message?: string;
}

/** Wire shape of `convert_files`' authoritative return value. */
export type ImageOutcome =
  | {
      kind: "completed";
      path: string;
      output_path: string;
      input_size_bytes: number;
      output_size_bytes: number;
      format: string;
      width: number;
      height: number;
    }
  | { kind: "failed"; path: string; error: { code: AppErrorCode; message: string } }
  | { kind: "canceled"; path: string };

export type QueueInvoke = (
  command: string,
  args?: Record<string, unknown>,
) => Promise<unknown>;

export type QueueListen = (
  event: string,
  handler: (payload: unknown) => void,
) => Promise<() => void>;

export interface QueueStoreOptions {
  invoke: QueueInvoke;
  listen: QueueListen;
  /** Jobs get fresh ids every run; injectable for deterministic tests. */
  makeJobId?: () => string;
  /** Command for batch processing (default: "hash_files"). */
  command?: string;
  /** Command for cancellation (default: "hash_cancel"). */
  cancelCommand?: string;
  /** Progress event name (default: "omnitools://hash-progress"). */
  progressEvent?: string;
}

export interface QueueStore {
  getState(): QueueSnapshot;
  subscribe(listener: () => void): () => void;
  /** Adds absolute paths; duplicates in the queue are ignored. */
  addPaths(paths: readonly string[]): void;
  /** Subscribes to progress events BEFORE invoking the batch. */
  run(extraArgs?: Record<string, unknown>): Promise<void>;
  cancel(): Promise<void>;
  /** Clears all rows (only while not running). */
  reset(): void;
}

let rowIdCounter = 0;

function basename(path: string): string {
  const parts = path.replace(/\\/g, "/").split("/");
  const last = parts[parts.length - 1];
  return last !== undefined && last.length > 0 ? last : path;
}

export function createQueueStore(options: QueueStoreOptions): QueueStore {
  // Required explicitly (runtime contract) — kept as a narrowed local so
  // strict TypeScript knows it is callable below.
  const makeJobId = options.makeJobId;
  if (typeof makeJobId !== "function") {
    throw new Error("createQueueStore requires an explicit makeJobId factory");
  }

  const command = options.command ?? HASH_COMMAND;
  const cancelCommand = options.cancelCommand ?? HASH_CANCEL_COMMAND;
  const progressEvent = options.progressEvent ?? HASH_PROGRESS_EVENT;

  let rows: QueueRow[] = [];
  let phase: JobPhase = "idle";
  let jobId: string | null = null;
  let snapshot: QueueSnapshot = emptySnapshot();
  const listeners = new Set<() => void>();

  const emit = () => {
    snapshot = { rows, phase, totals: computeTotals(rows) };
    for (const listener of listeners) listener();
  };

  const patchRow = (path: string, patch: (row: QueueRow) => QueueRow) => {
    rows = rows.map((row) => (row.path === path ? patch(row) : row));
  };

  const store: QueueStore = {
    getState(): QueueSnapshot {
      return snapshot;
    },
    subscribe(listener: () => void) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    addPaths(paths: readonly string[]) {
      const existing = new Set(rows.map((row) => row.path));
      for (const path of paths) {
        if (existing.has(path)) continue;
        existing.add(path);
        rowIdCounter += 1;
        rows = [
          ...rows,
          {
            id: `row-${rowIdCounter}`,
            path,
            name: basename(path),
            status: "idle",
          },
        ];
      }
      emit();
    },
    async run(extraArgs?: Record<string, unknown>): Promise<void> {
      if (phase === "running") return;
      const pendingRows = rows.filter((row) => row.status === "idle");
      if (pendingRows.length === 0) return;

      const runJobId = makeJobId();
      jobId = runJobId;
      phase = "running";
      emit();

      let unlisten: (() => void) | null = null;
      try {
        // Correction 3: attach the progress listener BEFORE invoking the
        // batch — early events can never be missed this way.
        unlisten = await options.listen(progressEvent, (payload) => {
          if (
            typeof payload === "object" &&
            payload !== null &&
            "phase" in payload &&
            (payload as ImageProgressPayload).jobId === runJobId
          ) {
            applyProgress(payload as ImageProgressPayload);
          }
        });

        const paths = pendingRows.map((row) => row.path);
        const outcomes = (await options.invoke(command, {
          jobId: runJobId,
          paths,
          ...extraArgs,
        })) as readonly ImageOutcome[];
        reconcile(pendingRows, outcomes, paths);
        phase = "finished";
      } catch (error) {
        // The invocation itself failed (interrupted, shell dead, closed
        // window). Rows must not hang as "running" forever.
        const toolError = parseToolError(error);
        for (const row of pendingRows) {
          if (row.status === "running" || row.status === "idle") {
            patchRow(row.path, (current) => ({
              ...current,
              status: "error",
              message:
                toolError.message ||
                `Hashing failed before this file reported (${describeAppErrorCode(toolError.code)}).`,
            }));
          }
        }
        phase = "finished";
      } finally {
        unlisten?.();
        jobId = null;
        emit();
      }
    },
    async cancel(): Promise<void> {
      if (phase !== "running" || jobId === null) return;
      try {
        await options.invoke(cancelCommand, { jobId });
      } catch {
        // A batch that already finished cannot be cancelled — safely ignored.
      }
    },
    reset() {
      if (phase === "running") return;
      rows = [];
      emit();
    },
  };

  const applyProgress = (event: HashProgressPayload | ImageProgressPayload) => {
    switch (event.phase) {
      case "started":
        patchRow(event.path, (row) => ({ ...row, status: "running" }));
        break;
      case "completed":
        patchRow(event.path, (row) => ({
          ...row,
          status: "done",
          // Hash progress uses hex/sizeBytes; image progress uses outputPath/outputSizeBytes.
          ...(event as ImageProgressPayload).outputPath !== undefined
            ? { outputPath: (event as ImageProgressPayload).outputPath }
            : { hex: (event as HashProgressPayload).hex },
          ...(event as ImageProgressPayload).outputSizeBytes !== undefined
            ? { sizeBytes: (event as ImageProgressPayload).outputSizeBytes }
            : { sizeBytes: (event as HashProgressPayload).sizeBytes },
          message: undefined,
        }));
        break;
      case "error":
        patchRow(event.path, (row) => ({
          ...row,
          status: "error",
          message: event.message,
        }));
        break;
      case "canceled":
        patchRow(event.path, (row) => ({ ...row, status: "canceled" }));
        break;
    }
    emit();
  };

  /** Final authoritative reconciliation from the command's return value. */
  const reconcile = (
    pendingRows: readonly QueueRow[],
    outcomes: readonly (HashOutcome | ImageOutcome)[],
    batchPaths: readonly string[],
  ) => {
    const reported = new Set<string>();

    for (const outcome of outcomes) {
      if (!batchPaths.includes(outcome.path)) continue;
      reported.add(outcome.path);
      patchRow(outcome.path, (row) => {
        if (outcome.kind === "completed") {
          if ("output_path" in outcome) {
            // ImageOutcome — output_path / output_size_bytes
            return {
              ...row,
              status: "done",
              outputPath: outcome.output_path,
              sizeBytes: outcome.output_size_bytes,
              message: undefined,
            };
          }
          // HashOutcome — hex / sizeBytes
          return { ...row, status: "done", hex: outcome.hex, sizeBytes: outcome.sizeBytes };
        }
        if (outcome.kind === "failed") {
          return {
            ...row,
            status: "error",
            message: outcome.error.message,
          };
        }
        return { ...row, status: "canceled", message: undefined };
      });
    }

    // Defense in depth: the Rust contract is one outcome per input. If a row
    // is somehow unreported, it is surfaced honestly instead of hanging.
    for (const row of pendingRows) {
      if (!reported.has(row.path) && (row.status === "idle" || row.status === "running")) {
        patchRow(row.path, (current) => ({
          ...current,
          status: "error",
          message: "The batch job did not report a result for this file.",
        }));
      }
    }
  };

  return store;
}

function emptySnapshot(): QueueSnapshot {
  return { rows: [], phase: "idle", totals: { total: 0, running: 0, done: 0, failed: 0, canceled: 0 } };
}

function computeTotals(rows: readonly QueueRow[]): QueueTotals {
  const totals: QueueTotals = { total: rows.length, running: 0, done: 0, failed: 0, canceled: 0 };
  for (const row of rows) {
    if (row.status === "running") totals.running += 1;
    else if (row.status === "done") totals.done += 1;
    else if (row.status === "error") totals.failed += 1;
    else if (row.status === "canceled") totals.canceled += 1;
  }
  return totals;
}

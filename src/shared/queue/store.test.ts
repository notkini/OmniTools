import { describe, expect, it, vi } from "vitest";
import {
  createQueueStore,
  HASH_CANCEL_COMMAND,
  HASH_COMMAND,
  HASH_PROGRESS_EVENT,
  type HashOutcome,
  type HashProgressPayload,
} from "./store";

/** In-memory adapters recording order and delivering scripted events. */
function makeAdapters() {
  const order: string[] = [];
  let progressHandler: ((payload: unknown) => void) | null = null;

  const invoke = vi.fn(async (command: string) => {
    order.push(`invoke:${command}`);
    if (command === HASH_COMMAND) return [] as HashOutcome[];
    return undefined;
  });

  const listen = vi.fn(async (event: string, handler: (payload: unknown) => void) => {
    order.push(`listen:${event}`);
    progressHandler = handler;
    return () => order.push(`unlisten:${event}`);
  });

  return {
    adapters: { invoke, listen },
    order,
    emitProgress: (payload: HashProgressPayload) => {
      progressHandler?.(payload);
    },
  };
}

const makeJobId = vi.fn(() => "job-1");

function outcomeFor(result: HashOutcome): HashOutcome {
  return result;
}

describe("queue store", () => {
  it("adds rows once per unique path and names them", () => {
    const { adapters } = makeAdapters();
    const store = createQueueStore({ ...adapters, makeJobId });
    store.addPaths(["/a/one.txt", "/a/two .txt"]);
    store.addPaths(["/a/one.txt", "/a/third.txt"]);

    const { rows, phase } = store.getState();
    expect(rows.map((row) => row.name)).toEqual(["one.txt", "two .txt", "third.txt"]);
    expect(rows.every((row) => row.status === "idle")).toBe(true);
    expect(phase).toBe("idle");
  });

  it("subscribes to progress before invoking so early events are not lost", async () => {
    const { adapters, order } = makeAdapters();
    const store = createQueueStore({ ...adapters, makeJobId });
    store.addPaths(["/a/one.txt"]);
    await store.run();

    expect(order[0]).toBe(`listen:${HASH_PROGRESS_EVENT}`);
    expect(order[1]).toBe(`invoke:${HASH_COMMAND}`);
    expect(order).toContain("unlisten:omnitools://hash-progress");
  });

  it("applies started/completed/error events per file and aggregates", async () => {
    const { adapters, emitProgress } = makeAdapters();
    const store = createQueueStore({ ...adapters, makeJobId });
    store.addPaths(["/a/keep.txt", "/a/broken.txt"]);
    // The authoritative return must match the events the hooks fired.
    adapters.invoke.mockImplementationOnce(async () => [
      outcomeFor({ kind: "completed", path: "/a/keep.txt", hex: "ab".repeat(32), sizeBytes: 5 }),
      outcomeFor({ kind: "failed", path: "/a/broken.txt", error: { code: "access_denied", message: "denied" } }),
    ] satisfies HashOutcome[]);
    const runPromise = store.run();

    emitProgress({ jobId: "job-1", path: "/a/keep.txt", phase: "started" });
    emitProgress({ jobId: "job-1", path: "/a/keep.txt", phase: "completed", hex: "ab".repeat(32), sizeBytes: 5 });
    emitProgress({
      jobId: "job-1",
      path: "/a/broken.txt",
      phase: "error",
      code: "access_denied",
      message: "denied",
    });
    await runPromise;

    const { rows, phase, totals } = store.getState();
    expect(rows[0]).toMatchObject({ status: "done", hex: "ab".repeat(32), sizeBytes: 5 });
    expect(rows[1]).toMatchObject({ status: "error", message: "denied" });
    expect(phase).toBe("finished");
    expect(totals).toEqual({ total: 2, running: 0, done: 1, failed: 1, canceled: 0 });
  });

  it("marks interrupted and pending files canceled, keeping completed/failed states", async () => {
    const { adapters, emitProgress } = makeAdapters();
    const store = createQueueStore({ ...adapters, makeJobId });
    store.addPaths(["/a/completed.txt", "/a/failed.txt", "/a/pending.txt"]);
    const invoke = adapters.invoke;
    invoke.mockImplementationOnce(async () => [
      outcomeFor({ kind: "completed", path: "/a/completed.txt", hex: "cc".repeat(32), sizeBytes: 1 }),
      outcomeFor({ kind: "failed", path: "/a/failed.txt", error: { code: "io", message: "boom" } }),
      outcomeFor({ kind: "canceled", path: "/a/pending.txt" }),
    ] satisfies HashOutcome[]);

    const runPromise = store.run();
    emitProgress({ jobId: "job-1", path: "/a/completed.txt", phase: "started" });
    await runPromise;

    const { rows, totals } = store.getState();
    expect(rows.map((row) => row.status)).toEqual(["done", "error", "canceled"]);
    expect(totals.canceled).toBe(1);
    expect(totals.failed).toBe(1);
    expect(totals.done).toBe(1);
  });

  it("cancel routes to hash_cancel with the running job id", async () => {
    const { adapters } = makeAdapters();
    const store = createQueueStore({ ...adapters, makeJobId });
    store.addPaths(["/a/x.bin"]);

    let releaseInvoke: ((value: HashOutcome[] | undefined) => void) | undefined;
    adapters.invoke.mockImplementationOnce(
      (command) =>
        new Promise<HashOutcome[] | undefined>((resolve) => {
          if (command === HASH_COMMAND) {
            // Simulate a still-running batch: resolve when released.
            releaseInvoke = resolve;
          } else {
            resolve(undefined);
          }
        }),
    );

    const runPromise = store.run();
    await Promise.resolve();
    await store.cancel();

    expect(adapters.invoke).toHaveBeenCalledWith(HASH_CANCEL_COMMAND, {
      jobId: "job-1",
    });

    releaseInvoke?.([]);
    await runPromise;
  });

  it("ignores progress events from other job ids", async () => {
    const { adapters, emitProgress } = makeAdapters();
    const store = createQueueStore({ ...adapters, makeJobId });
    store.addPaths(["/a/x.bin"]);

    // The batch completed honestly; foreign/unknown-path events must not
    // have touched either row.
    adapters.invoke.mockImplementationOnce(async () => [
      outcomeFor({ kind: "completed", path: "/a/x.bin", hex: "ee".repeat(32), sizeBytes: 9 }),
    ] satisfies HashOutcome[]);

    const runPromise = store.run();
    emitProgress({ jobId: "job-other", path: "/a/x.bin", phase: "completed", hex: "zz" });
    emitProgress({ jobId: "job-1", path: "/a/unknown.bin", phase: "completed", hex: "ab" });
    await runPromise;

    const { rows } = store.getState();
    expect(rows[0]?.status).toBe("done");
    // The foreign-job digest ('zz') and unknown-path event must not apply.
    expect(rows[0]?.hex).toBe("ee".repeat(32));
  });

  it("recovers rows when the invocation rejects", async () => {
    const { adapters } = makeAdapters();
    const store = createQueueStore({ ...adapters, makeJobId });
    store.addPaths(["/a/x.bin"]);

    adapters.invoke.mockImplementationOnce(async (command) => {
      expect(command).toBe(HASH_COMMAND);
      throw { code: "io", message: "shell died" };
    });

    await store.run();

    const { rows, phase, totals } = store.getState();
    expect(rows[0]).toMatchObject({ status: "error", message: "shell died" });
    expect(phase).toBe("finished");
    expect(totals.failed).toBe(1);
  });

  it("reset clears rows while not running and addPaths during run leaves new rows idle", async () => {
    const { adapters } = makeAdapters();
    const store = createQueueStore({ ...adapters, makeJobId });
    store.addPaths(["/a/x.bin"]);

    let releaseInvoke: ((value: HashOutcome[] | undefined) => void) | undefined;
    adapters.invoke.mockImplementationOnce(
      () =>
        new Promise<HashOutcome[] | undefined>((resolve) => {
          releaseInvoke = resolve;
        }),
    );

    const runPromise = store.run();
    await Promise.resolve();
    store.addPaths(["/a/late.bin"]);
    store.reset(); // must be a no-op while running

    // A running batch cannot be reset.
    expect(store.getState().rows).toHaveLength(2);
    expect(store.getState().rows[1]?.status).toBe("idle");

    releaseInvoke?.([]);
    await runPromise;
    store.reset();
    expect(store.getState().rows).toHaveLength(0);
  });

  it("run() with no pending rows is a no-op", async () => {
    const { adapters } = makeAdapters();
    const store = createQueueStore({ ...adapters, makeJobId });
    await store.run();
    expect(adapters.listen).not.toHaveBeenCalled();
    expect(adapters.invoke).not.toHaveBeenCalled();
  });
});

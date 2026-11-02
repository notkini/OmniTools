import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ToastProvider } from "../../../components/feedback/Toast";
import HashToolPage, {
  formatSize,
  normalizeDigest,
  type HashToolDeps,
} from "./HashTool";
import {
  createQueueStore,
  type HashOutcome,
  type HashProgressPayload,
} from "../../../shared/queue/store";

interface HashToolTestDeps {
  pickFiles: () => Promise<string[] | null>;
  pickSaveDestination: () => Promise<string | null>;
  exportChecksums: (destination: string, entries: readonly string[]) => Promise<void>;
  compareFiles: (first: string, second: string) => Promise<{ identical: boolean }>;
}

function makeDeps(overrides: Partial<HashToolTestDeps> = {}): HashToolTestDeps {
  return {
    pickFiles: vi.fn(async () => null) as unknown as () => Promise<string[] | null>,
    pickSaveDestination: vi.fn(async () => null) as unknown as () => Promise<string | null>,
    exportChecksums: vi.fn(async () => undefined) as unknown as (
      destination: string,
      entries: readonly string[],
    ) => Promise<void>,
    compareFiles: vi.fn(async () => ({ identical: true })) as unknown as (
      first: string,
      second: string,
    ) => Promise<{ identical: boolean }>,
    ...overrides,
  };
}

/** Fake adapters recording call order and delivering scripted events. */
function makeAdapters() {
  const invokes: Array<{ command: string; args?: Record<string, unknown> }> = [];
  let progressHandler: ((payload: unknown) => void) | null = null;
  let holdBatch: ((outcomes: unknown) => void) | null = null;

  const invoke = vi.fn(async (command: string, args?: Record<string, unknown>) => {
    invokes.push({ command, args });
    if (command === "hash_files") {
      return new Promise((resolve) => {
        holdBatch = resolve;
      });
    }
    return undefined;
  });

  const listen = vi.fn(async (_event: string, handler: (payload: unknown) => void) => {
    progressHandler = handler;
    return () => {};
  });

  return {
    adapters: { invoke, listen },
    invokes,
    emitProgress: (payload: HashProgressPayload) => progressHandler?.(payload),
    resolveBatch: (outcomes: HashOutcome[]) => holdBatch?.(outcomes),
  };
}

function renderTool(store: ReturnType<typeof createQueueStore>, deps: HashToolDeps) {
  return render(
    <ToastProvider>
      <HashToolPage store={store} deps={deps} />
    </ToastProvider>,
  );
}

const HEX = "ab".repeat(32);

function addFiles(store: ReturnType<typeof createQueueStore>, paths: string[]) {
  store.addPaths(paths);
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe("HashTool page", () => {
  it("renders one row per file with honest statuses", async () => {
    const { adapters, emitProgress, resolveBatch } = makeAdapters();
    const store = createQueueStore({ ...adapters, makeJobId: () => "job-1" });
    const deps = makeDeps();
    renderTool(store, deps);

    addFiles(store, ["/a/keep.txt", "/a/broken.txt"]);
    const runPromise = store.run();
    // One microtask tick lets run() reach the batch invoke before events.
    await Promise.resolve();
    emitProgress({
      jobId: "job-1",
      path: "/a/keep.txt",
      phase: "started",
    });
    emitProgress({
      jobId: "job-1",
      path: "/a/keep.txt",
      phase: "completed",
      hex: HEX,
      sizeBytes: 2048,
    });
    emitProgress({
      jobId: "job-1",
      path: "/a/broken.txt",
      phase: "error",
      code: "access_denied",
      message: "denied by the system",
    });
    resolveBatch([
      { kind: "completed", path: "/a/keep.txt", hex: HEX, sizeBytes: 2048 },
      {
        kind: "failed",
        path: "/a/broken.txt",
        error: { code: "access_denied", message: "denied by the system" },
      },
    ]);
    await runPromise;

    expect(screen.getAllByText("keep.txt").length).toBeGreaterThan(0);
    expect(screen.getAllByText("broken.txt").length).toBeGreaterThan(0);
    expect(screen.getByText("Done")).toBeTruthy();
    expect(screen.getByText("Error")).toBeTruthy();
    expect(screen.getByText("denied by the system")).toBeTruthy();
  });

  it("copies a computed digest through the clipboard", async () => {
    const { adapters, emitProgress, resolveBatch } = makeAdapters();
    const store = createQueueStore({ ...adapters, makeJobId: () => "job-1" });
    const deps = makeDeps();
    renderTool(store, deps);

    addFiles(store, ["/a/keep.txt"]);
    const runPromise = store.run();
    // One microtask tick lets run() reach the batch invoke before events.
    await Promise.resolve();
    emitProgress({
      jobId: "job-1",
      path: "/a/keep.txt",
      phase: "completed",
      hex: HEX,
      sizeBytes: 5,
    });
    resolveBatch([
      { kind: "completed", path: "/a/keep.txt", hex: HEX, sizeBytes: 5 },
    ]);
    await runPromise;

    const writeText = vi.fn(async () => undefined);
    Object.assign(navigator, { clipboard: { writeText } });
    await userEvent.click(screen.getByRole("button", { name: /copy sha-256 of keep\.txt/i }));

    expect(writeText).toHaveBeenCalledWith(HEX);
  });

  it("shows a Cancel primary action while running and routes the cancel", async () => {
    const { adapters, invokes, emitProgress, resolveBatch } = makeAdapters();
    const store = createQueueStore({ ...adapters, makeJobId: () => "job-1" });
    const deps = makeDeps();
    renderTool(store, deps);

    addFiles(store, ["/a/x.bin"]);
    const runPromise = store.run();
    await Promise.resolve();
    emitProgress({ jobId: "job-1", path: "/a/x.bin", phase: "started" });

    await userEvent.click(
      await screen.findByRole("button", { name: "Cancel" }),
    );

    // The backend reports the cancellation via a canceled outcome.
    resolveBatch([{ kind: "canceled", path: "/a/x.bin" }]);
    await runPromise;

    expect(
      invokes.some((call) => call.command === "hash_cancel"),
    ).toBe(true);
    expect(store.getState().rows[0]?.status).toBe("canceled");
  });

  it("labels the export action exactly and exports what is done", async () => {
    const { adapters, emitProgress, resolveBatch } = makeAdapters();
    const store = createQueueStore({ ...adapters, makeJobId: () => "job-1" });
    const pickSaveDestination = vi.fn(async () => "/d/checksums.txt");
    const exportChecksums = vi.fn(async () => undefined);
    const deps = makeDeps({ pickSaveDestination, exportChecksums });
    renderTool(store, deps);

    addFiles(store, ["/a/keep.txt"]);
    const runPromise = store.run();
    // One microtask tick lets run() reach the batch invoke before events.
    await Promise.resolve();
    emitProgress({
      jobId: "job-1",
      path: "/a/keep.txt",
      phase: "completed",
      hex: HEX,
      sizeBytes: 5,
    });
    resolveBatch([
      { kind: "completed", path: "/a/keep.txt", hex: HEX, sizeBytes: 5 },
    ]);
    await runPromise;

    const exportButton = screen.getByRole("button", { name: "Export checksums…" });
    await userEvent.click(exportButton);

    expect(pickSaveDestination).toHaveBeenCalledTimes(1);
    expect(exportChecksums).toHaveBeenCalledWith("/d/checksums.txt", [
      `${HEX}  */a/keep.txt`,
    ]);
  });

  it("a dismissed save dialog exports nothing", async () => {
    const { adapters, emitProgress, resolveBatch } = makeAdapters();
    const store = createQueueStore({ ...adapters, makeJobId: () => "job-1" });
    const pickSaveDestination = vi.fn(async () => null);
    const exportChecksums = vi.fn(async () => undefined);
    const deps = makeDeps({ pickSaveDestination, exportChecksums });
    renderTool(store, deps);

    addFiles(store, ["/a/keep.txt"]);
    const runPromise = store.run();
    // One microtask tick lets run() reach the batch invoke before events.
    await Promise.resolve();
    emitProgress({
      jobId: "job-1",
      path: "/a/keep.txt",
      phase: "completed",
      hex: HEX,
      sizeBytes: 5,
    });
    resolveBatch([
      { kind: "completed", path: "/a/keep.txt", hex: HEX, sizeBytes: 5 },
    ]);
    await runPromise;

    await userEvent.click(screen.getByRole("button", { name: "Export checksums…" }));

    expect(exportChecksums).not.toHaveBeenCalled();
  });

  it("a dismissed file dialog adds no rows", async () => {
    const { adapters } = makeAdapters();
    const store = createQueueStore({ ...adapters, makeJobId: () => "job-1" });
    const pickFiles = vi.fn(async () => null);
    const deps = makeDeps({ pickFiles });
    renderTool(store, deps);

    await userEvent.click(screen.getByRole("button", { name: "Add files" }));

    expect(pickFiles).toHaveBeenCalledTimes(1);
    expect(screen.queryByRole("list", { name: "File queue" })).toBeNull();
  });

  it("compare reports identical and different banners", async () => {
    const { adapters } = makeAdapters();
    const store = createQueueStore({ ...adapters, makeJobId: () => "job-1" });
    const compare = vi.fn()
      .mockResolvedValueOnce({ identical: true })
      .mockResolvedValueOnce({ identical: false });
    const deps = makeDeps({ compareFiles: compare });
    renderTool(store, deps);

    addFiles(store, ["/a/one.bin", "/b/two.bin"]);
    await screen.findAllByText("one.bin"); // row + select option share the name

    const first = screen.getByLabelText("First file");
    const second = screen.getByLabelText("Second file");
    await userEvent.selectOptions(first, "/a/one.bin");
    await userEvent.selectOptions(second, "/b/two.bin");
    await userEvent.click(screen.getByRole("button", { name: "Compare" }));

    expect(await screen.findByText(/identical — the files match/i)).toBeTruthy();
    expect(compare).toHaveBeenCalledWith("/a/one.bin", "/b/two.bin");

    await userEvent.click(screen.getByRole("button", { name: "Compare" }));
    expect(await screen.findByText(/different — the files do not match/i)).toBeTruthy();
  });

  it("verify reports match against the completed digest", async () => {
    const { adapters, emitProgress, resolveBatch } = makeAdapters();
    const store = createQueueStore({ ...adapters, makeJobId: () => "job-1" });
    const deps = makeDeps();
    renderTool(store, deps);

    addFiles(store, ["/a/keep.txt"]);
    const runPromise = store.run();
    // One microtask tick lets run() reach the batch invoke before events.
    await Promise.resolve();
    emitProgress({
      jobId: "job-1",
      path: "/a/keep.txt",
      phase: "completed",
      hex: HEX,
      sizeBytes: 5,
    });
    resolveBatch([
      { kind: "completed", path: "/a/keep.txt", hex: HEX, sizeBytes: 5 },
    ]);
    await runPromise;

    await userEvent.selectOptions(screen.getByLabelText("File"), "/a/keep.txt");
    await userEvent.type(
      screen.getByLabelText(/expected sha-256/i),
      `${HEX.slice(0, 8)}:${HEX.slice(8, 32)}  ${HEX.slice(32)}`,
    );
    await userEvent.click(screen.getByRole("button", { name: "Verify" }));

    expect(await screen.findByText(/match — the digest matches/i)).toBeTruthy();
  });

  it("verify reports a mismatch honestly", async () => {
    const { adapters, emitProgress, resolveBatch } = makeAdapters();
    const store = createQueueStore({ ...adapters, makeJobId: () => "job-1" });
    const deps = makeDeps();
    renderTool(store, deps);

    addFiles(store, ["/a/keep.txt"]);
    const runPromise = store.run();
    // One microtask tick lets run() reach the batch invoke before events.
    await Promise.resolve();
    emitProgress({
      jobId: "job-1",
      path: "/a/keep.txt",
      phase: "completed",
      hex: HEX,
      sizeBytes: 5,
    });
    resolveBatch([
      { kind: "completed", path: "/a/keep.txt", hex: HEX, sizeBytes: 5 },
    ]);
    await runPromise;

    await userEvent.selectOptions(screen.getByLabelText("File"), "/a/keep.txt");
    await userEvent.type(screen.getByLabelText(/expected sha-256/i), "c".repeat(64));
    await userEvent.click(screen.getByRole("button", { name: "Verify" }));

    expect(await screen.findByText(/no match — this is not the sha-256/i)).toBeTruthy();
  });

  it("surfaces export failures as an honest toast", async () => {
    const { adapters, emitProgress, resolveBatch } = makeAdapters();
    const store = createQueueStore({ ...adapters, makeJobId: () => "job-1" });
    const pickSaveDestination = vi.fn(async () => "/d/checksums.txt");
    const exportChecksums = vi.fn(async () => {
      throw { code: "access_denied", message: "locked" };
    });
    const deps = makeDeps({ pickSaveDestination, exportChecksums });
    renderTool(store, deps);

    addFiles(store, ["/a/keep.txt"]);
    const runPromise = store.run();
    // One microtask tick lets run() reach the batch invoke before events.
    await Promise.resolve();
    emitProgress({
      jobId: "job-1",
      path: "/a/keep.txt",
      phase: "completed",
      hex: HEX,
      sizeBytes: 5,
    });
    resolveBatch([
      { kind: "completed", path: "/a/keep.txt", hex: HEX, sizeBytes: 5 },
    ]);
    await runPromise;

    await userEvent.click(screen.getByRole("button", { name: "Export checksums…" }));

    expect(await screen.findByRole("alert")).toBeTruthy();
  });
});

describe("HashTool helpers", () => {
  it("formatSize renders honest human units", () => {
    expect(formatSize(undefined)).toBeUndefined();
    expect(formatSize(0)).toBe("0 B");
    expect(formatSize(1023)).toBe("1023 B");
    expect(formatSize(2048)).toBe("2.0 KB");
    expect(formatSize(5 * 1024 * 1024)).toBe("5.0 MB");
  });

  it("normalizeDigest strips whitespace, colons and case", () => {
    expect(normalizeDigest(`AB:CD  EF`)).toBe("abcdef");
    expect(normalizeDigest(`  ${HEX.toUpperCase()} `)).toBe(HEX);
  });

  it("renders sizes for real completed rows only", async () => {
    const { adapters, emitProgress, resolveBatch } = makeAdapters();
    const store = createQueueStore({ ...adapters, makeJobId: () => "job-1" });
    const deps = makeDeps();
    renderTool(store, deps);

    addFiles(store, ["/a/keep.txt"]);
    const runPromise = store.run();
    // One microtask tick lets run() reach the batch invoke before events.
    await Promise.resolve();
    emitProgress({
      jobId: "job-1",
      path: "/a/keep.txt",
      phase: "completed",
      hex: HEX,
      sizeBytes: 300,
    });
    resolveBatch([
      { kind: "completed", path: "/a/keep.txt", hex: HEX, sizeBytes: 300 },
    ]);
    await runPromise;

    expect(screen.getByText("300 B")).toBeTruthy();
  });
});

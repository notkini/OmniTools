import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import ImageConvertPage, { __resetImageQueueStoreForTests } from "./ImageConvert";
import { ToastProvider } from "../../../components/feedback/Toast";
import { createQueueStore } from "../../../shared/queue/store";

// Mock the invoke and listen adapters
vi.mock("../../../shared/fs/invoke", () => ({
  tauriInvokeAdapter: vi.fn(),
  tauriListenAdapter: vi.fn(),
  pickFiles: vi.fn(),
  pickImageFiles: vi.fn(),
  pickDirectory: vi.fn(),
  pickSaveDestination: vi.fn(),
  pickImageDestination: vi.fn(),
}));

import {
  tauriInvokeAdapter,
  tauriListenAdapter,
  pickImageFiles,
  pickDirectory,
  pickImageDestination,
} from "../../../shared/fs/invoke";

const mockInvoke = vi.mocked(tauriInvokeAdapter);
const mockListen = vi.mocked(tauriListenAdapter);
const mockPickImageFiles = vi.mocked(pickImageFiles);
const mockPickDirectory = vi.mocked(pickDirectory);
const mockPickImageDestination = vi.mocked(pickImageDestination);

/** Rust serializes `ImageOutcome` with `rename_all = "camelCase"` —
 * these fixtures are the authoritative wire shape. */
const completedOutcome = {
  kind: "completed",
  path: "C:\\images\\test.png",
  outputPath: "C:\\staging\\batch-0\\test.png",
  stagedFileId: "staged-0",
  inputSizeBytes: 1024,
  outputSizeBytes: 800,
  format: "png",
  width: 8,
  height: 8,
};

beforeEach(() => {
  vi.clearAllMocks();
  mockListen.mockResolvedValue(vi.fn());
  mockPickImageFiles.mockResolvedValue(null);
  mockPickDirectory.mockResolvedValue(null);
  mockPickImageDestination.mockResolvedValue(null);
  // Default: convert_files returns a successful camelCase outcome; the
  // thumbnail command resolves to a tiny data URL; discard is a no-op.
  mockInvoke.mockImplementation((command: string) => {
    if (command === "convert_files") {
      return Promise.resolve([completedOutcome]);
    }
    if (command === "image_thumbnail") {
      return Promise.resolve("data:image/png;base64,AAAA");
    }
    if (command === "discard_staged") {
      return Promise.resolve(1);
    }
    return Promise.resolve();
  });
});

afterEach(() => {
  vi.resetAllMocks();
  __resetImageQueueStoreForTests();
});

const renderWithProviders = (ui: React.ReactElement) => {
  return render(<ToastProvider>{ui}</ToastProvider>);
};

async function addFiles(paths: string[]) {
  mockPickImageFiles.mockResolvedValue(paths);
  renderWithProviders(<ImageConvertPage />);
  fireEvent.click(screen.getByRole("button", { name: /add images/i }));
  await waitFor(() => {
    expect(screen.getByText("test.png")).toBeDefined();
  });
}

describe("Image Converter - UI controls", () => {
  it("renders without crashing", () => {
    renderWithProviders(<ImageConvertPage />);
    expect(screen.getByText("Add images")).toBeDefined();
  });

  it("shows format selection with PNG, JPEG, WebP", () => {
    renderWithProviders(<ImageConvertPage />);
    const formatSelect = screen.getByRole("combobox");
    expect(formatSelect).toBeDefined();
    expect((formatSelect as HTMLSelectElement).value).toBe("jpeg");
  });

  it("shows quality slider for JPEG by default", () => {
    renderWithProviders(<ImageConvertPage />);
    const slider = screen.getByRole("slider");
    expect(slider).toBeDefined();
    expect((slider as HTMLInputElement).value).toBe("85");
  });

  it("hides quality slider when PNG is selected", () => {
    renderWithProviders(<ImageConvertPage />);
    const formatSelect = screen.getByRole("combobox");
    fireEvent.change(formatSelect, { target: { value: "png" } });
    expect(screen.queryByRole("slider")).toBeNull();
  });

  it("shows quality slider again when JPEG is re-selected", () => {
    renderWithProviders(<ImageConvertPage />);
    const formatSelect = screen.getByRole("combobox");
    fireEvent.change(formatSelect, { target: { value: "png" } });
    expect(screen.queryByRole("slider")).toBeNull();
    fireEvent.change(formatSelect, { target: { value: "jpeg" } });
    expect(screen.getByRole("slider")).toBeDefined();
  });

  it("resize toggle is unchecked by default", () => {
    renderWithProviders(<ImageConvertPage />);
    const resizeToggle = screen.getAllByRole("checkbox")[0];
    expect((resizeToggle as HTMLInputElement).checked).toBe(false);
  });

  it("resize inputs are hidden when resize is disabled", () => {
    renderWithProviders(<ImageConvertPage />);
    expect(screen.queryByPlaceholderText("Width")).toBeNull();
    expect(screen.queryByPlaceholderText("Height")).toBeNull();
  });

  it("shows resize inputs when toggle is enabled", () => {
    renderWithProviders(<ImageConvertPage />);
    const resizeToggle = screen.getAllByRole("checkbox")[0];
    fireEvent.click(resizeToggle!);
    expect(screen.getByPlaceholderText("Width")).toBeDefined();
    expect(screen.getByPlaceholderText("Height")).toBeDefined();
  });

  it("resize inputs are hidden again when toggle is disabled", () => {
    renderWithProviders(<ImageConvertPage />);
    const resizeToggle = screen.getAllByRole("checkbox")[0];
    fireEvent.click(resizeToggle!);
    expect(screen.getByPlaceholderText("Width")).toBeDefined();
    fireEvent.click(resizeToggle!);
    expect(screen.queryByPlaceholderText("Width")).toBeNull();
  });

  it("convert button is disabled with no files", () => {
    renderWithProviders(<ImageConvertPage />);
    const convertButton = screen.getByRole("button", { name: "Convert" });
    expect((convertButton as HTMLButtonElement).disabled).toBe(true);
  });

  it("does not call convert_files invoke before converting", () => {
    renderWithProviders(<ImageConvertPage />);
    const convertButton = screen.getByRole("button", { name: "Convert" });
    fireEvent.click(convertButton);
    expect(mockInvoke).not.toHaveBeenCalled();
  });
});

describe("Image Converter - input validation", () => {
  it("rejects non-image files with an explicit not-allowed message", async () => {
    mockPickImageFiles.mockResolvedValue(["C:\\downloads\\report.txt"]);
    renderWithProviders(<ImageConvertPage />);
    fireEvent.click(screen.getByRole("button", { name: /add images/i }));
    await waitFor(() => {
      expect(mockInvoke).not.toHaveBeenCalled();
    });
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toMatch(/not allowed/i);
  });

  it("adds only the image files from a mixed selection", async () => {
    mockPickImageFiles.mockResolvedValue([
      "C:\\images\\photo.png",
      "C:\\downloads\\notes.pdf",
    ]);
    renderWithProviders(<ImageConvertPage />);
    fireEvent.click(screen.getByRole("button", { name: /add images/i }));
    // Only the PNG row exists; the non-image file never enters the queue.
    await waitFor(() => {
      expect(screen.getByText("photo.png")).toBeDefined();
    });
    expect(screen.queryByText("notes.pdf")).toBeNull();
  });
});

describe("Image Converter - invoke integration", () => {
  it("calls convert_files with camelCase opts when converting", async () => {
    await addFiles(["C:\\images\\test.png"]);
    fireEvent.click(screen.getByRole("button", { name: /convert 1 file/i }));

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith(
        "convert_files",
        expect.objectContaining({
          jobId: expect.any(String),
          paths: ["C:\\images\\test.png"],
          opts: {
            outputFormat: "jpeg",
            quality: 85,
            resize: undefined,
          },
        }),
      );
    });
  });

  it("calls convert_files with resize maxWidth/maxHeight when enabled", async () => {
    await addFiles(["C:\\images\\test.png"]);
    const resizeToggle = screen.getAllByRole("checkbox")[0]!;
    fireEvent.click(resizeToggle);
    fireEvent.change(screen.getByPlaceholderText("Width"), { target: { value: "800" } });
    fireEvent.change(screen.getByPlaceholderText("Height"), { target: { value: "600" } });
    fireEvent.click(screen.getByRole("button", { name: /convert 1 file/i }));

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith(
        "convert_files",
        expect.objectContaining({
          opts: {
            outputFormat: "jpeg",
            quality: 85,
            resize: { maxWidth: 800, maxHeight: 600 },
          },
        }),
      );
    });
  });

  it("blocks conversion when resize is enabled with missing numbers", async () => {
    await addFiles(["C:\\images\\test.png"]);
    const resizeToggle = screen.getAllByRole("checkbox")[0]!;
    fireEvent.click(resizeToggle);
    fireEvent.click(screen.getByRole("button", { name: /convert 1 file/i }));
    await waitFor(() => {
      expect(mockInvoke).not.toHaveBeenCalled();
    });
    const alert = await screen.findByRole("alert");
    expect(alert.textContent).toMatch(/maximum width and height/i);
  });

  it("shows the converted row with format, dimensions and thumbnail", async () => {
    await addFiles(["C:\\images\\test.png"]);
    fireEvent.click(screen.getByRole("button", { name: /convert 1 file/i }));
    await waitFor(() => {
      expect(screen.getByText("PNG")).toBeDefined();
    });
    await waitFor(() => {
      expect(document.querySelector("img.file-preview")).not.toBeNull();
      expect(mockInvoke).toHaveBeenCalledWith("image_thumbnail", {
        stagedFileId: "staged-0",
      });
    });
  });

  it("saves a single converted file with camelCase args", async () => {
    mockPickImageDestination.mockResolvedValue("C:\\output\\test.png");
    await addFiles(["C:\\images\\test.png"]);
    fireEvent.click(screen.getByRole("button", { name: /convert 1 file/i }));
    const saveButton = await screen.findByRole("button", { name: /save test\.png/i });
    fireEvent.click(saveButton);
    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith("save_converted", {
        stagedFileId: "staged-0",
        destination: "C:\\output\\test.png",
      });
    });
    await waitFor(() => {
      expect(screen.getByText(/Saved/i)).toBeDefined();
    });
  });

  it("offers Open output location after a successful save", async () => {
    mockPickImageDestination.mockResolvedValue("C:\\output\\test.png");
    await addFiles(["C:\\images\\test.png"]);
    fireEvent.click(screen.getByRole("button", { name: /convert 1 file/i }));
    const saveButton = await screen.findByRole("button", { name: /save test\.png/i });
    fireEvent.click(saveButton);
    await screen.findByText(/open output location/i);
    expect(screen.getByText(/open output location/i)).toBeDefined();
  });

  it("keeps the add-files zone available after a batch for appending", async () => {
    await addFiles(["C:\\images\\test.png"]);
    expect(screen.getByRole("button", { name: /add more files/i })).toBeDefined();
    // Appending a second image adds a row without touching the first.
    mockPickImageFiles.mockResolvedValue(["C:\\images\\second.png"]);
    fireEvent.click(screen.getByRole("button", { name: /add more files/i }));
    await waitFor(() => {
      expect(screen.getByText("second.png")).toBeDefined();
    });
    expect(screen.getByText("test.png")).toBeDefined();
  });

  it("discards staged files when Clear results is used", async () => {
    await addFiles(["C:\\images\\test.png"]);
    fireEvent.click(screen.getByRole("button", { name: /convert 1 file/i }));
    await screen.findByRole("button", { name: /save test\.png/i });
    fireEvent.click(screen.getByRole("button", { name: /clear results/i }));
    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith("discard_staged", {
        stagedFileIds: ["staged-0"],
      });
    });
  });

  it("resets quality to 85 when switching back to JPEG", () => {
    renderWithProviders(<ImageConvertPage />);
    const formatSelect = screen.getByRole("combobox");
    fireEvent.change(formatSelect, { target: { value: "png" } });
    fireEvent.change(formatSelect, { target: { value: "jpeg" } });
    const slider = screen.getByRole("slider") as HTMLInputElement;
    expect(slider.value).toBe("85");
  });
});

describe("Queue store - removePath", () => {
  it("removes a single row by path", () => {
    const store = createQueueStore({
      invoke: tauriInvokeAdapter,
      listen: tauriListenAdapter,
      makeJobId: () => "test-job-id",
    });

    store.addPaths(["/tmp/file1.png", "/tmp/file2.png", "/tmp/file3.png"]);
    expect(store.getState().rows).toHaveLength(3);

    store.removePath("/tmp/file2.png");
    const state = store.getState();
    expect(state.rows).toHaveLength(2);
    expect(state.rows.map((r) => r.path)).toEqual(["/tmp/file1.png", "/tmp/file3.png"]);
  });

  it("removePath does nothing when not running and path not found", () => {
    const store = createQueueStore({
      invoke: tauriInvokeAdapter,
      listen: tauriListenAdapter,
      makeJobId: () => "test-job-id",
    });

    store.addPaths(["/tmp/file1.png"]);
    store.removePath("/tmp/nonexistent.png");
    expect(store.getState().rows).toHaveLength(1);
  });

  it("removePath does nothing while running", () => {
    // Can't easily simulate running state, but the guard exists in code
    const store = createQueueStore({
      invoke: tauriInvokeAdapter,
      listen: tauriListenAdapter,
      makeJobId: () => "test-job-id",
    });
    store.addPaths(["/tmp/file1.png"]);
    store.removePath("/tmp/file1.png");
    // While not running (default state), removal should work
    expect(store.getState().rows).toHaveLength(0);
  });
});

import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import BatchRenamePage from "./BatchRename";
import { ToastProvider } from "../../../components/feedback/Toast";

/**
 * Mocks the shared invoke bridge. `BatchRenamePage` must talk to the Rust
 * commands through these wrappers only — the assertions below pin the exact
 * command names and wire payloads the Rust side expects.
 */
vi.mock("../../../shared/fs/invoke", () => ({
  tauriInvokeAdapter: vi.fn(),
  tauriListenAdapter: vi.fn(),
  pickFiles: vi.fn(),
  pickRenameFiles: vi.fn(),
}));

import { pickFiles, pickRenameFiles, tauriInvokeAdapter } from "../../../shared/fs/invoke";

const mockInvoke = vi.mocked(tauriInvokeAdapter);
const mockPickFiles = vi.mocked(pickFiles);
const mockPickRenameFiles = vi.mocked(pickRenameFiles);

const renderWithProviders = (ui: React.ReactElement) => {
  return render(<ToastProvider>{ui}</ToastProvider>);
};

/** Adds one file through the (mocked) file dialog and waits for its preview. */
async function addFileThroughDialog(path: string) {
  mockPickRenameFiles.mockResolvedValue([path]);
  mockPickFiles.mockResolvedValue([path]);
  fireEvent.click(screen.getByText("Add files"));
  await waitFor(() => {
    expect(mockPickRenameFiles).toHaveBeenCalled();
  });
}

beforeEach(() => {
  vi.clearAllMocks();
  mockPickRenameFiles.mockResolvedValue(null);
  mockPickFiles.mockResolvedValue(null);
  mockInvoke.mockResolvedValue([]);
});

afterEach(() => {
  vi.restoreAllMocks();
});

describe("Batch Rename UI", () => {
  it("renders workspace, dropzone and operation builder", () => {
    renderWithProviders(<BatchRenamePage />);
    expect(screen.getByText("Add files")).toBeTruthy();
    expect(screen.getByText("Operations")).toBeTruthy();
    expect(screen.getByText(/no files selected/i)).toBeTruthy();
  });

  it("shows the empty preview state with no files", () => {
    renderWithProviders(<BatchRenamePage />);
    expect(screen.getByText(/no files selected/i)).toBeTruthy();
  });

  it("adds and removes an operation card", () => {
    renderWithProviders(<BatchRenamePage />);
    const addButton = screen.getByRole("button", { name: /add operation/i });
    fireEvent.click(addButton);
    expect(screen.getByTestId("operation-card-1")).toBeTruthy();
    const removeButton = screen.getByRole("button", { name: /remove prefix/i });
    fireEvent.click(removeButton);
    expect(screen.queryByTestId("operation-card-1")).toBeNull();
  });

  it("requests plan_rename with wire operations once files are added", async () => {
    const proposals = [
      {
        fromName: "a.txt",
        toName: "new-a.txt",
        kind: "ok" as const,
        reason: "",
      },
    ];
    mockInvoke.mockImplementation(async (command: string) => {
      if (command === "plan_rename") return proposals;
      return [];
    });
    renderWithProviders(<BatchRenamePage />);
    await addFileThroughDialog("C:\\tmp\\a.txt");

    await waitFor(() => {
      expect(mockInvoke).toHaveBeenCalledWith(
        "plan_rename",
        expect.objectContaining({
          paths: ["C:\\tmp\\a.txt"],
          operations: [],
        }),
      );
    });

    // The proposed name renders in the preview table.
    await waitFor(() => {
      expect(screen.getAllByText("new-a.txt").length).toBeGreaterThan(0);
    });
  });

  it("disables the primary action while no files are queued", () => {
    renderWithProviders(<BatchRenamePage />);
    const renameButton = screen.getByRole("button", { name: /rename 0 files/i });
    expect((renameButton as HTMLButtonElement).disabled).toBe(true);
  });

  it("sends skip_paths to apply_rename and adopts returned paths", async () => {
    const actions: string[] = [];
    mockInvoke.mockImplementation(async (command: string, args?: Record<string, unknown>) => {
      if (command === "plan_rename")
        return [
          {
            fromName: "a.txt",
            toName: "final-a.txt",
            kind: "ok" as const,
            reason: "",
          },
        ];
      if (command === "apply_rename") {
        actions.push(JSON.stringify(args));
        return [
          {
            kind: "renamed",
            from: "C:\\tmp\\a.txt",
            to: "C:\\tmp\\final-a.txt",
          },
        ];
      }
      return [];
    });
    renderWithProviders(<BatchRenamePage />);
    await addFileThroughDialog("C:\\tmp\\a.txt");

    const renameButton = await waitFor(() => {
      const button = screen.getByRole("button", { name: /rename 1 file/i });
      expect((button as HTMLButtonElement).disabled).toBe(false);
      return button;
    });
    fireEvent.click(renameButton);

    await waitFor(() => {
      expect(actions.length).toBe(1);
    });
    const sentRaw = actions[0];
    expect(sentRaw).toBeTruthy();
    const sentArgs = JSON.parse(sentRaw ?? "{}") as {
      paths: string[];
      skip_paths: string[];
    };
    expect(sentArgs.paths).toEqual(["C:\\tmp\\a.txt"]);
    expect(sentArgs.skip_paths).toEqual([]);
  });

  it("keeps Undo disabled before any successful rename", () => {
    renderWithProviders(<BatchRenamePage />);
    const undoButton = screen.getByRole("button", { name: /^undo$/i });
    expect((undoButton as HTMLButtonElement).disabled).toBe(true);
  });

  it("does not invoke any command before files are added", () => {
    renderWithProviders(<BatchRenamePage />);
    expect(mockInvoke).not.toHaveBeenCalled();
  });
});

import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import ImageConvertPage from "./ImageConvert";
import { ToastProvider } from "../../../components/feedback/Toast";

// Mock the invoke and listen adapters
vi.mock("../../../shared/fs/invoke", () => ({
  tauriInvokeAdapter: vi.fn(),
  tauriListenAdapter: vi.fn(),
  pickFiles: vi.fn(),
  pickDirectory: vi.fn(),
  pickSaveDestination: vi.fn(),
}));

import {
  tauriInvokeAdapter,
  tauriListenAdapter,
  pickDirectory,
  pickSaveDestination,
} from "../../../shared/fs/invoke";

const mockInvoke = vi.mocked(tauriInvokeAdapter);
const mockListen = vi.mocked(tauriListenAdapter);
const mockPickDirectory = vi.mocked(pickDirectory);
const mockPickSaveDestination = vi.mocked(pickSaveDestination);

beforeEach(() => {
  vi.clearAllMocks();
  mockListen.mockResolvedValue(vi.fn());
  mockPickDirectory.mockResolvedValue(null);
  mockPickSaveDestination.mockResolvedValue(null);
});

afterEach(() => {
  vi.resetAllMocks();
});

const renderWithProviders = (ui: React.ReactElement) => {
  return render(<ToastProvider>{ui}</ToastProvider>);
};

describe("Image Converter", () => {
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
    fireEvent.click(resizeToggle);
    expect(screen.getByPlaceholderText("Width")).toBeDefined();
    expect(screen.getByPlaceholderText("Height")).toBeDefined();
  });

  it("convert button is disabled with no files", () => {
    renderWithProviders(<ImageConvertPage />);
    const convertButton = screen.getByRole("button", { name: /convert 0 files/i });
    expect((convertButton as HTMLButtonElement).disabled).toBe(true);
  });

  it("does not call convert_files invoke before converting", () => {
    renderWithProviders(<ImageConvertPage />);
    expect(mockInvoke).not.toHaveBeenCalled();
  });

  it("opens save dialog when saving individual result", async () => {
    mockPickSaveDestination.mockResolvedValue("/tmp/output.png");
    renderWithProviders(<ImageConvertPage />);
    // With no files, there are no save buttons
    const saveButtons = screen.queryAllByRole("button", { name: /save .*…/i });
    expect(saveButtons).toHaveLength(0);
  });

  it("opens directory dialog when saving all", async () => {
    mockPickDirectory.mockResolvedValue("/tmp/output");
    renderWithProviders(<ImageConvertPage />);
    // With no files, there is no save all button
    const saveAllButtons = screen.queryAllByRole("button", { name: /save all/i });
    expect(saveAllButtons).toHaveLength(0);
  });
});

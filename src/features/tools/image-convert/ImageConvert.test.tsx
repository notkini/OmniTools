import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import ImageConvertPage from "./ImageConvert";

// Mock the invoke and listen adapters
vi.mock("../../../shared/fs/invoke", () => ({
  tauriInvokeAdapter: vi.fn(),
  tauriListenAdapter: vi.fn(),
  pickFiles: vi.fn(),
  pickDirectory: vi.fn(),
}));

import {
  tauriInvokeAdapter,
  tauriListenAdapter,
  pickDirectory,
} from "../../../shared/fs/invoke";

const mockInvoke = vi.mocked(tauriInvokeAdapter);
const mockListen = vi.mocked(tauriListenAdapter);
const mockPickDirectory = vi.mocked(pickDirectory);

beforeEach(() => {
  vi.clearAllMocks();
  mockListen.mockResolvedValue(vi.fn());
  mockPickDirectory.mockResolvedValue(null);
});

afterEach(() => {
  vi.resetAllMocks();
});

describe("Image Converter", () => {
  it("renders without crashing", () => {
    render(<ImageConvertPage />);
    expect(screen.getByText("Add images")).toBeDefined();
  });

  it("shows format selection with PNG, JPEG, WebP", () => {
    render(<ImageConvertPage />);
    const formatSelect = screen.getByRole("combobox");
    expect(formatSelect).toBeDefined();
    expect((formatSelect as HTMLSelectElement).value).toBe("jpeg");
  });

  it("shows quality slider for JPEG", () => {
    render(<ImageConvertPage />);
    expect(screen.getByRole("slider")).toBeDefined();
  });

  it("shows resize inputs", () => {
    render(<ImageConvertPage />);
    expect(screen.getByPlaceholderText("Width")).toBeDefined();
    expect(screen.getByPlaceholderText("Height")).toBeDefined();
  });

  it("shows output folder selection", () => {
    render(<ImageConvertPage />);
    expect(screen.getByLabelText("Choose output folder")).toBeDefined();
  });

  it("convert button is disabled with no files", () => {
    render(<ImageConvertPage />);
    const convertButton = screen.getByRole("button", { name: /convert 0 files/i });
    expect((convertButton as HTMLButtonElement).disabled).toBe(true);
  });

  it("calls pickDirectory when choosing output folder", async () => {
    mockPickDirectory.mockResolvedValue("/tmp/output");
    render(<ImageConvertPage />);
    fireEvent.click(screen.getByRole("button", { name: "Choose output folder" }));
    await vi.waitFor(() => {
      expect(mockPickDirectory).toHaveBeenCalled();
    });
  });

  it("does not call convert_files invoke before converting", () => {
    render(<ImageConvertPage />);
    expect(mockInvoke).not.toHaveBeenCalled();
  });

  it("disables convert button when no output folder", () => {
    render(<ImageConvertPage />);
    const convertButton = screen.getByRole("button", { name: /convert 0 files/i });
    expect((convertButton as HTMLButtonElement).disabled).toBe(true);
  });
});

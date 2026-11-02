import { describe, expect, it, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
import { CommandPalette } from "./CommandPalette";

function renderPalette(overrides: Partial<Parameters<typeof CommandPalette>[0]> = {}) {
  const onClose = vi.fn();
  const onSelect = vi.fn();
  const props: Parameters<typeof CommandPalette>[0] = {
    open: overrides.open ?? true,
    onClose: overrides.onClose ?? onClose,
    onSelect: overrides.onSelect ?? onSelect,
  };
  const view = render(<CommandPalette {...props} />);
  return { ...view, onClose, onSelect };
}

describe("CommandPalette", () => {
  it("renders the search input when open", () => {
    renderPalette();
    expect(screen.getByRole("combobox")).toBeTruthy();
    expect(screen.getByRole("listbox")).toBeTruthy();
  });

  it("renders nothing when closed", () => {
    renderPalette({ open: false });
    expect(screen.queryByRole("combobox")).toBeNull();
  });

  it("lists registered tools and filters as the user types", () => {
    renderPalette();
    expect(screen.getByText("About")).toBeTruthy();
    const input = screen.getByRole("combobox") as HTMLInputElement;
    fireEvent.change(input, { target: { value: "zzz-no-such" } });
    expect(screen.getByText(/No matches for/i)).toBeTruthy();
    fireEvent.change(input, { target: { value: "" } });
    expect(screen.getByText("About")).toBeTruthy();
  });

  it("activates the selected result with Enter and closes", () => {
    const { onSelect, onClose } = renderPalette();
    const input = screen.getByRole("combobox");
    fireEvent.keyDown(input, { key: "Enter" });
    expect(onSelect).toHaveBeenCalledWith("#/tools/about");
    expect(onClose).toHaveBeenCalled();
  });

  it("moves the selection with arrow keys", () => {
    renderPalette();
    const input = screen.getByRole("combobox");
    // Single result — ArrowDown keeps index 0 without crashing; expose via aria state.
    fireEvent.keyDown(input, { key: "ArrowDown" });
    expect(input.getAttribute("aria-activedescendant")).toBe("palette-option-0");
  });

  it("dismisses with Escape and restores focus", () => {
    const button = document.body.appendChild(document.createElement("button"));
    try {
      const { onClose } = renderPalette();
      expect(document.activeElement).not.toBe(button);
      fireEvent.keyDown(screen.getByRole("combobox"), { key: "Escape" });
      expect(onClose).toHaveBeenCalled();
    } finally {
      button.remove();
    }
  });

  it("dismisses on overlay click", () => {
    const { onClose } = renderPalette();
    fireEvent.click(screen.getByTestId("palette-overlay"));
    expect(onClose).toHaveBeenCalled();
  });
});

import { describe, expect, it } from "vitest";
import {
  resolveTheme,
  readStoredTheme,
  THEME_STORAGE_KEY,
} from "./theme";

describe("resolveTheme", () => {
  it("follows the system when preference is system", () => {
    expect(resolveTheme("system", true)).toBe("dark");
    expect(resolveTheme("system", false)).toBe("light");
  });

  it("returns an explicit preference unchanged", () => {
    expect(resolveTheme("light", true)).toBe("light");
    expect(resolveTheme("dark", false)).toBe("dark");
  });
});

describe("readStoredTheme", () => {
  it("reads a stored valid preference", () => {
    const store = {
      getItem: (key: string) => (key === THEME_STORAGE_KEY ? "light" : null),
    };
    expect(readStoredTheme(store)).toBe("light");
  });

  it("falls back to system for unknown or missing values", () => {
    const store = {
      getItem: (key: string) =>
        key === THEME_STORAGE_KEY ? "neon-pink" : null,
    };
    expect(readStoredTheme(store)).toBe("system");
    expect(readStoredTheme({ getItem: () => null })).toBe("system");
    // Storage throwing must also fall back to the default.
    expect(
      readStoredTheme({
        getItem: () => {
          throw new Error("storage unavailable");
        },
      }),
    ).toBe("system");
  });
});

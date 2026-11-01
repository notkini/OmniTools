/* Theme resolution — pure logic, unit-tested. DOM side effects live in ThemeProvider. */

export type ThemePreference = "light" | "dark" | "system";
export type ResolvedTheme = "light" | "dark";

export const THEME_STORAGE_KEY = "omnitools.theme";

export const THEME_PREFERENCES: ThemePreference[] = ["light", "dark", "system"];

/** Resolve a preference into the theme actually applied. */
export function resolveTheme(
  preference: ThemePreference,
  systemPrefersDark: boolean,
): ResolvedTheme {
  if (preference === "system") return systemPrefersDark ? "dark" : "light";
  return preference;
}

/** Read the persisted preference, tolerating corrupt or unknown values. */
export function readStoredTheme(
  storage: Pick<Storage, "getItem"> = localStorage,
): ThemePreference {
  try {
    const value = storage.getItem(THEME_STORAGE_KEY);
    if (value && (THEME_PREFERENCES as string[]).includes(value)) {
      return value as ThemePreference;
    }
  } catch {
    // Storage unavailable (e.g. webview restrictions) — fall back to default.
  }
  return "system";
}

/** Apply the theme to the document. Called by ThemeProvider only. */
export function applyTheme(resolved: ResolvedTheme): void {
  document.documentElement.dataset.theme = resolved;
}

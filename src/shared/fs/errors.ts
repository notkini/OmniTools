/**
 * Frontend mirror of the Rust `AppErrorCode` enum (src-tauri/src/error.rs).
 * A closed, exhaustive set — adding a Rust variant requires updating this
 * list and every `switch` below. That exhaustiveness is the contract.
 */
export const APP_ERROR_CODES = [
  "not_found",
  "access_denied",
  "is_a_directory",
  "canceled",
  "invalid_input",
  "io",
] as const;

export type AppErrorCode = (typeof APP_ERROR_CODES)[number];

export function isAppErrorCode(value: unknown): value is AppErrorCode {
  return (
    typeof value === "string" &&
    (APP_ERROR_CODES as readonly string[]).includes(value)
  );
}

/** ToolError — thrown when a Rust command rejects with a known AppError. */
export class ToolError extends Error {
  readonly code: AppErrorCode;

  constructor(code: AppErrorCode, message: string) {
    super(message);
    this.name = "ToolError";
    this.code = code;
  }
}

/** Parses any rejection into a ToolError; unknown shapes become honest `io`. */
export function parseToolError(value: unknown): ToolError {
  if (value instanceof ToolError) return value;
  if (typeof value === "object" && value !== null) {
    const candidate = value as { code?: unknown; message?: unknown };
    if (isAppErrorCode(candidate.code)) {
      const message =
        typeof candidate.message === "string" ? candidate.message : "";
      return new ToolError(candidate.code, message);
    }
  }
  if (value instanceof Error) {
    return new ToolError("io", value.message);
  }
  return new ToolError("io", String(value));
}

/** Exhaustive, human-readable text per code (rendered in the UI). */
export function describeAppErrorCode(code: AppErrorCode): string {
  switch (code) {
    case "not_found":
      return "The file no longer exists at that location.";
    case "access_denied":
      return "Access was denied — the file may be locked or protected.";
    case "is_a_directory":
      return "That path is a directory, not a file.";
    case "canceled":
      return "Cancelled.";
    case "invalid_input":
      return "The request was invalid.";
    case "io":
      return "A filesystem error occurred.";
  }
}

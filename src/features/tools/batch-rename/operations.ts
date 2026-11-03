/**
 * Pure helpers for the Rename operation builder.
 *
 * All functions here are framework-free and unit-testable. The frontend never
 * implements rename logic itself — it only builds the serializable
 * `RenameOperation` payloads that the Rust `plan_rename` command consumes.
 */

export type OperationType =
  | "prefix"
  | "suffix"
  | "replace"
  | "numbering"
  | "case"
  | "extension"
  | "pattern";

export type CaseMode = "upper" | "lower" | "title";

export interface RenameOperationDraft {
  id: string;
  op:
    | { type: "prefix"; value: string }
    | { type: "suffix"; value: string }
    | { type: "replace"; find: string; replaceWith: string; caseSensitive: boolean }
    | { type: "numbering"; start: number; step: number; padding: number }
    | { type: "case"; mode: CaseMode }
    | { type: "extension"; value: string }
    | { type: "pattern"; template: string };
}

export type OperationDraft = RenameOperationDraft;

export function makeOperationId(): string {
  if (typeof crypto !== "undefined" && typeof crypto.randomUUID === "function") {
    return crypto.randomUUID();
  }
  return `${Date.now()}-${Math.random().toString(36).slice(2)}`;
}

export function makeOperation(type: OperationType): RenameOperationDraft {
  switch (type) {
    case "prefix":
      return { id: makeOperationId(), op: { type: "prefix", value: "" } };
    case "suffix":
      return { id: makeOperationId(), op: { type: "suffix", value: "" } };
    case "replace":
      return {
        id: makeOperationId(),
        op: { type: "replace", find: "", replaceWith: "", caseSensitive: false },
      };
    case "numbering":
      return {
        id: makeOperationId(),
        op: { type: "numbering", start: 1, step: 1, padding: 1 },
      };
    case "case":
      return { id: makeOperationId(), op: { type: "case", mode: "upper" } };
    case "extension":
      return { id: makeOperationId(), op: { type: "extension", value: "" } };
    case "pattern":
      return { id: makeOperationId(), op: { type: "pattern", template: "" } };
  }
}

export function operationLabel(type: OperationType): string {
  switch (type) {
    case "prefix":
      return "Prefix";
    case "suffix":
      return "Suffix";
    case "replace":
      return "Find & replace";
    case "numbering":
      return "Number";
    case "case":
      return "Case";
    case "extension":
      return "Extension";
    case "pattern":
      return "Pattern";
  }
}

export function operationHint(type: OperationType): string {
  switch (type) {
    case "prefix":
      return "Text added before the filename";
    case "suffix":
      return "Text added before the file extension";
    case "replace":
      return "Find text and replace it";
    case "numbering":
      return "Add sequential numbers to filenames";
    case "case":
      return "Change letter case of the filename";
    case "extension":
      return "Change the file extension only";
    case "pattern":
      return "Build a filename from tokens: {n}, {orig}, {date}";
  }
}

export function moveOperation<T>(list: T[], from: number, to: number): T[] {
  const result = [...list];
  const [removed] = result.splice(from, 1);
  result.splice(to, 0, removed);
  return result;
}

export function validateNumbering(
  start: number | undefined,
  step: number | undefined,
  padding: number | undefined,
): { start: number; step: number; padding: number } {
  const s = start == null || Number.isNaN(start) ? 1 : Math.max(1, Math.floor(start));
  const st = step == null || Number.isNaN(step) ? 1 : Math.max(1, Math.floor(step));
  const p = padding == null || Number.isNaN(padding) ? 0 : Math.max(0, Math.min(20, Math.floor(padding)));
  return { start: s, step: st, padding: p };
}

export function toWireOperations(drafts: RenameOperationDraft[]): RenameOperationDraft["op"][] {
  return drafts.map((d) => d.op);
}

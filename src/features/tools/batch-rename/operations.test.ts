import { describe, it, expect } from "vitest";
import {
  makeOperation,
  makeOperationId,
  makeOperationValue,
  moveOperation,
  operationHint,
  operationLabel,
  toWireOperations,
  validateNumbering,
  type OperationDraft,
} from "./operations";

describe("makeOperationValue", () => {
  it("creates the wire default per type", () => {
    expect(makeOperationValue("prefix")).toEqual({ type: "prefix", value: "" });
    expect(makeOperationValue("suffix")).toEqual({ type: "suffix", value: "" });
    expect(makeOperationValue("replace")).toEqual({
      type: "replace",
      find: "",
      replaceWith: "",
      caseSensitive: false,
    });
    expect(makeOperationValue("numbering")).toEqual({
      type: "numbering",
      start: 1,
      step: 1,
      padding: 1,
    });
    expect(makeOperationValue("case")).toEqual({ type: "case", mode: "upper" });
    expect(makeOperationValue("extension")).toEqual({
      type: "extension",
      value: "",
    });
    expect(makeOperationValue("pattern")).toEqual({
      type: "pattern",
      template: "",
    });
  });
});

describe("makeOperation", () => {
  it("wraps the wire default with a fresh id", () => {
    const op = makeOperation("prefix");
    expect(op.op.type).toBe("prefix");
    expect(op).toEqual({ id: op.id, op: makeOperationValue("prefix") });
    expect(typeof op.id).toBe("string");
    expect(op.id.length).toBeGreaterThan(0);
  });

  it("generates unique ids", () => {
    expect(makeOperationId()).not.toBe(makeOperationId());
  });
});

describe("operationLabel", () => {
  it("returns correct labels", () => {
    expect(operationLabel("prefix")).toBe("Prefix");
    expect(operationLabel("suffix")).toBe("Suffix");
    expect(operationLabel("replace")).toBe("Find & replace");
    expect(operationLabel("numbering")).toBe("Number");
    expect(operationLabel("case")).toBe("Case");
    expect(operationLabel("extension")).toBe("Extension");
    expect(operationLabel("pattern")).toBe("Pattern");
  });
});

describe("operationHint", () => {
  it("returns hints", () => {
    expect(operationHint("prefix")).toContain("before");
    expect(operationHint("replace")).toContain("replace");
    expect(operationHint("pattern")).toContain("{n}");
  });
});

describe("moveOperation", () => {
  it("moves item up", () => {
    expect(moveOperation(["a", "b", "c"], 1, 0)).toEqual(["b", "a", "c"]);
  });

  it("moves item down", () => {
    expect(moveOperation(["a", "b", "c"], 0, 1)).toEqual(["b", "a", "c"]);
  });

  it("keeps order for identical indices", () => {
    expect(moveOperation(["a", "b"], 1, 1)).toEqual(["a", "b"]);
  });

  it("does nothing for out-of-range indices", () => {
    expect(moveOperation(["a", "b"], 0, 5)).toEqual(["a", "b"]);
    expect(moveOperation(["a", "b"], 5, 0)).toEqual(["a", "b"]);
    expect(moveOperation(["a", "b"], -1, 0)).toEqual(["a", "b"]);
  });
});

describe("validateNumbering", () => {
  it("returns defaults for undefined values", () => {
    expect(validateNumbering(undefined, undefined, undefined)).toEqual({
      start: 1,
      step: 1,
      padding: 0,
    });
  });

  it("clamps start to minimum 1", () => {
    expect(validateNumbering(0, 1, 0).start).toBe(1);
    expect(validateNumbering(-5, 1, 0).start).toBe(1);
    expect(validateNumbering(5, 1, 0).start).toBe(5);
  });

  it("clamps step to minimum 1", () => {
    expect(validateNumbering(1, 0, 0).step).toBe(1);
    expect(validateNumbering(1, -3, 0).step).toBe(1);
    expect(validateNumbering(1, 5, 0).step).toBe(5);
  });

  it("clamps padding between 0 and 20", () => {
    expect(validateNumbering(1, 1, -1).padding).toBe(0);
    expect(validateNumbering(1, 1, 25).padding).toBe(20);
    expect(validateNumbering(1, 1, 3).padding).toBe(3);
  });
});

describe("toWireOperations", () => {
  it("extracts op objects from drafts", () => {
    const drafts: OperationDraft[] = [
      { id: "a", op: { type: "prefix", value: "pre_" } },
      { id: "b", op: { type: "suffix", value: "_post" } },
    ];
    expect(toWireOperations(drafts)).toEqual([
      { type: "prefix", value: "pre_" },
      { type: "suffix", value: "_post" },
    ]);
  });
});

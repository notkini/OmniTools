import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import {
  makeOperation,
  makeOperationId,
  moveOperation,
  toWireOperations,
  validateNumbering,
  type OperationDraft,
  type OperationType,
} from "./operations";

describe("makeOperation", () => {
  it("creates prefix operation", () => {
    const op = makeOperation("prefix");
    expect(op.op.type).toBe("prefix");
    expect(op.op.value).toBe("");
    expect(typeof op.id).toBe("string");
    expect(op.id.length).toBeGreaterThan(0);
  });

  it("creates suffix operation", () => {
    const op = makeOperation("suffix");
    expect(op.op.type).toBe("suffix");
    expect(op.op.value).toBe("");
  });

  it("creates replace operation", () => {
    const op = makeOperation("replace");
    expect(op.op.type).toBe("replace");
    expect(op.op.find).toBe("");
    expect(op.op.replaceWith).toBe("");
    expect(op.op.caseSensitive).toBe(false);
  });

  it("creates numbering operation", () => {
    const op = makeOperation("numbering");
    expect(op.op.type).toBe("numbering");
    expect(op.op.start).toBe(1);
    expect(op.op.step).toBe(1);
    expect(op.op.padding).toBe(1);
  });

  it("creates case operation", () => {
    const op = makeOperation("case");
    expect(op.op.type).toBe("case");
    expect(op.op.mode).toBe("upper");
  });

  it("creates extension operation", () => {
    const op = makeOperation("extension");
    expect(op.op.type).toBe("extension");
    expect(op.op.value).toBe("");
  });

  it("creates pattern operation", () => {
    const op = makeOperation("pattern");
    expect(op.op.type).toBe("pattern");
    expect(op.op.template).toBe("");
  });

  it("generates unique ids", () => {
    const a = makeOperationId();
    const b = makeOperationId();
    expect(a).not.toBe(b);
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
    expect(operationHint("prefix")).toContain("Prefix");
    expect(operationHint("replace")).toContain("replace");
    expect(operationHint("pattern")).toContain("{n}");
  });
});

describe("moveOperation", () => {
  it("moves item up", () => {
    const list = ["a", "b", "c"];
    expect(moveOperation(list, 1, 0)).toEqual(["b", "a", "c"]);
  });

  it("moves item down", () => {
    const list = ["a", "b", "c"];
    expect(moveOperation(list, 0, 1)).toEqual(["b", "a", "c"]);
  });

  it("does nothing for out-of-range indices", () => {
    const list = ["a", "b"];
    expect(moveOperation(list, 0, 5)).toEqual(["a", "b"]);
    expect(moveOperation(list, 5, 0)).toEqual(["a", "b"]);
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
    const wire = toWireOperations(drafts);
    expect(wire).toEqual([
      { type: "prefix", value: "pre_" },
      { type: "suffix", value: "_post" },
    ]);
  });
});

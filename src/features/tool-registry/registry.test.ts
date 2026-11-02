import { describe, expect, it } from "vitest";
import { getCategoryLabel, getTool, TOOLS } from "./registry";

describe("registry integrity", () => {
  it("has at least one registered tool", () => {
    expect(TOOLS.length).toBeGreaterThan(0);
  });

  it("uses unique, non-empty ids", () => {
    const ids = TOOLS.map((tool) => tool.id);
    expect(ids.length).toBe(new Set(ids).size);
    for (const id of ids) expect(id.length).toBeGreaterThan(0);
  });

  it("derives routePath purely from the id", () => {
    for (const tool of TOOLS) {
      expect(tool.routePath).toBe(`/tools/${tool.id}`);
    }
  });

  it("only uses categories that expose a label", () => {
    for (const tool of TOOLS) {
      expect(getCategoryLabel(tool.category).length).toBeGreaterThan(0);
    }
  });

  it("provides a component loader and search keywords for every tool", () => {
    for (const tool of TOOLS) {
      expect(typeof tool.load).toBe("function");
      expect(tool.keywords.length).toBeGreaterThan(0);
    }
  });

  it("resolves lookups", () => {
    expect(getTool("about")).toBeDefined();
    expect(getTool("does-not-exist")).toBeUndefined();
  });
});

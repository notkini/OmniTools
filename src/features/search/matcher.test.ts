import { describe, expect, it } from "vitest";
import { searchTools } from "./matcher";
import type { ToolDefinition } from "../tool-registry/types";

function makeTool(id: string, name: string, keywords: readonly string[]): ToolDefinition {
  return {
    id,
    name,
    description: `${id} description`,
    category: "system",
    icon: {} as unknown as ToolDefinition["icon"],
    keywords,
    routePath: `#/tools/${id}`,
    load: async () => ({ default: () => null }),
  };
}

const A = makeTool("a", "File Hasher", ["sha256", "hash", "checksum"]);
const B = makeTool("b", "Batch Rename", ["rename", "bulk"]);
const C = makeTool("c", "About", ["version", "privacy"]);
const TOOLS = [A, B, C];

describe("searchTools", () => {
  it("returns all tools in registry order for an empty query", () => {
    expect(searchTools("", TOOLS)).toEqual([A, B, C]);
    expect(searchTools("   ", TOOLS)).toEqual([A, B, C]);
  });

  it("finds a tool by exact name", () => {
    expect(searchTools("file hasher", TOOLS)).toEqual([A]);
  });

  it("ranks keyword matches and ranks the stronger word-boundary hit first", () => {
    expect(searchTools("rename", TOOLS)).toEqual([B]);
    // "ha" hits "Hasher" (word-boundary, name ×2) harder than "Batch" (sparse).
    expect(searchTools("ha", TOOLS)).toEqual([A, B]);
  });

  it("matches keywords too", () => {
    expect(searchTools("sha256", TOOLS)).toEqual([A]);
    expect(searchTools("privacy", TOOLS)).toEqual([C]);
  });

  it("returns nothing for non-matching queries", () => {
    expect(searchTools("zzzz", TOOLS)).toEqual([]);
  });

  it("is deterministic across repeated calls", () => {
    expect(searchTools("ha", TOOLS)).toEqual(searchTools("ha", TOOLS));
  });
});

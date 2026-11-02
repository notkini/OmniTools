import { describe, expect, it } from "vitest";
import type { ToolDefinition } from "../features/tool-registry/types";
import { hashToPath, parseHash } from "./router";

const aboutTool: ToolDefinition = {
  id: "about",
  name: "About",
  description: "info",
  category: "system",
  icon: {} as unknown as ToolDefinition["icon"],
  keywords: ["about"],
  routePath: "/tools/about",
  load: async () => ({ default: () => null }),
};

const getTool = (id: string) => (id === "about" ? aboutTool : undefined);

describe("hashToPath", () => {
  it("normalises empty and fragment-only hashes to home", () => {
    expect(hashToPath("")).toBe("/");
    expect(hashToPath("#")).toBe("/");
    expect(hashToPath("#/")).toBe("/");
    expect(hashToPath("//")).toBe("/");
  });

  it("keeps meaningful paths intact", () => {
    expect(hashToPath("#/settings")).toBe("/settings");
    expect(hashToPath("#/tools/about")).toBe("/tools/about");
  });
});

describe("parseHash", () => {
  it("parses home", () => {
    expect(parseHash("#/", getTool)).toEqual({ kind: "home" });
    expect(parseHash("", getTool)).toEqual({ kind: "home" });
  });

  it("parses settings", () => {
    expect(parseHash("#/settings", getTool)).toEqual({ kind: "settings" });
  });

  it("parses a registered tool route and carries the tool definition", () => {
    const route = parseHash("#/tools/about", getTool);
    expect(route).toEqual({ kind: "tool", tool: aboutTool });
  });

  it("returns notFound for unknown tool slugs — never silent", () => {
    const route = parseHash("#/tools/missing", getTool);
    expect(route).toEqual({ kind: "notFound", path: "/tools/missing" });
  });

  it("returns notFound for arbitrary hash junk, preserving the path", () => {
    expect(parseHash("#/junk/rubbish", getTool)).toEqual({
      kind: "notFound",
      path: "/junk/rubbish",
    });
  });

  it("does not treat subpaths as tool routes", () => {
    expect(parseHash("#/tools/about/edit", getTool).kind).toBe("notFound");
  });
});

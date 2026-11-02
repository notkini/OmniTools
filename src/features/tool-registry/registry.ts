import type { ToolCategory, ToolCategoryId, ToolDefinition } from "./types";

/**
 * Category labels — only categories with real content exist.
 * New tools extend this map together with ToolCategoryId in types.ts.
 */
const CATEGORY_LABELS: Record<ToolCategoryId, string> = {
  system: "System",
};

export function getCategoryLabel(id: ToolCategoryId): ToolCategory["label"] {
  return CATEGORY_LABELS[id];
}

/**
 * Tool registration: one import line per tool + one array entry.
 * Nothing else in the app touches tool modules.
 */
import { tool as aboutTool } from "../tools/about";

export const TOOLS: ToolDefinition[] = [aboutTool];

export function getTool(id: string): ToolDefinition | undefined {
  return TOOLS.find((tool) => tool.id === id);
}

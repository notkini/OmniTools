import type { LucideIcon } from "lucide-react";
import type { ComponentType } from "react";

/** Stable identifier — also the URL slug: `/tools/<id>`. Never rename an id. */
export type ToolId = string;

export type ToolRoutePath = `/tools/${ToolId}`;

/**
 * Topics used for sidebar grouping and search readout. Extended only when a
 * real tool needs a new one — never pre-created empty.
 */
export type ToolCategoryId = "system" | "files";

export type ToolCategory = {
  id: ToolCategoryId;
  label: string;
};

export type ToolComponentLoader = () => Promise<{ default: ComponentType }>;

/**
 * The one contract every tool implements. The registry is the single source
 * of truth: sidebar, home, router, and command palette all derive from this —
 * no tool metadata is duplicated anywhere else.
 */
export interface ToolDefinition {
  id: ToolId;
  name: string;
  description: string;
  category: ToolCategoryId;
  icon: LucideIcon;
  keywords: readonly string[];
  routePath: ToolRoutePath;
  /** Lazy-loaded page component (code-split per future tool size). */
  load: ToolComponentLoader;
}

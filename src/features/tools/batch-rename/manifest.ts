import { FileText } from "lucide-react";
import type { ToolDefinition } from "../../tool-registry/types";
import BatchRenamePage from "./BatchRename";

export const tool: ToolDefinition = {
  id: "batch-rename",
  name: "Batch Rename",
  description:
    "Rename multiple files at once with find/replace, numbering, case change, and more",
  category: "files",
  icon: FileText,
  keywords: ["rename", "batch", "files", "mass", "pattern", "extension"],
  routePath: "/tools/batch-rename",
  load: async () => ({ default: BatchRenamePage }),
};

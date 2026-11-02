import { Hash } from "lucide-react";
import type { ToolDefinition } from "../../tool-registry/types";
import HashToolPage from "./HashTool";

export const tool: ToolDefinition = {
  id: "file-hash",
  name: "File Hasher",
  description:
    "SHA-256 checksums for one file or a batch, with compare and verify",
  category: "files",
  icon: Hash,
  keywords: ["sha256", "hash", "checksum", "verify", "compare", "digest"],
  routePath: "/tools/file-hash",
  load: async () => ({ default: HashToolPage }),
};

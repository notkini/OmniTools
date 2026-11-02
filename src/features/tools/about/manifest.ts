import { Info } from "lucide-react";
import type { ToolDefinition } from "../../tool-registry/types";
import AboutPage from "./AboutPage";

export const tool: ToolDefinition = {
  id: "about",
  name: "About",
  description: "Version, licence, and privacy information",
  category: "system",
  icon: Info,
  keywords: ["version", "licence", "license", "privacy", "offline"],
  routePath: "/tools/about",
  load: async () => ({ default: AboutPage }),
};

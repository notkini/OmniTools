import { Image } from "lucide-react";
import type { ToolDefinition } from "../../tool-registry/types";
import ImageConvertPage from "./ImageConvert";

export const tool: ToolDefinition = {
  id: "image-convert",
  name: "Image Converter",
  description: "Convert, resize, and compress PNG/JPEG/WebP images locally",
  category: "files",
  icon: Image,
  keywords: ["convert", "image", "resize", "compress", "png", "jpeg", "webp"],
  routePath: "/tools/image-convert",
  load: async () => ({ default: ImageConvertPage }),
};

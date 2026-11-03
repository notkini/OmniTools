/**
 * Typed frontend-to-Rust bridge. The web layer never touches the filesystem
 * directly — only through these invoke wrappers (AGENTS.md architecture rule).
 */
import { invoke as tauriInvoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { open, save } from "@tauri-apps/plugin-dialog";
import { parseToolError } from "./errors";

/** The minimal invoke surface the queue store depends on (mockable). */
export type QueueInvoke = (
  command: string,
  args?: Record<string, unknown>,
) => Promise<unknown>;

/** The minimal event-listen surface the queue store depends on (mockable). */
export type QueueListen = (
  event: string,
  handler: (payload: unknown) => void,
) => Promise<() => void>;

export const tauriInvokeAdapter: QueueInvoke = async (command, args) => {
  try {
    return (await tauriInvoke(command, args)) as unknown;
  } catch (error) {
    throw parseToolError(error);
  }
};

export const tauriListenAdapter: QueueListen = (event, handler) =>
  listen(event, (event) => handler(event.payload));

/**
 * Native multi-file selection. Returns absolute paths, or null when the
 * dialog was dismissed — dismissing must never enqueue rows.
 */
export async function pickFiles(): Promise<string[] | null> {
  const selection = await open({
    multiple: true,
    title: "Choose files to hash",
  });
  if (selection === null) return null;
  return Array.isArray(selection) ? selection : [selection];
}

/** Native save dialog for checksum export. Null = dismissed. */
export async function pickSaveDestination(): Promise<string | null> {
  return save({
    title: "Export checksums…",
    defaultPath: "checksums.txt",
    filters: [{ name: "Checksum file", extensions: ["txt"] }],
  });
}

/** Native directory selection for output destination. Null = dismissed. */
export async function pickDirectory(): Promise<string | null> {
  const selection = await open({
    directory: true,
    title: "Choose output folder",
  });
  if (selection === null) return null;
  return Array.isArray(selection) ? selection[0] : selection;
}

/** Image filter offering the native dialog. `All files` stays selectable so
 * mislabeled or wrong-type files can still be picked — the tool then reports
 * them honestly instead of the OS silently hiding them. */
const IMAGE_FILTERS = [
  { name: "Images (PNG · JPEG · WebP)", extensions: ["png", "jpg", "jpeg", "webp"] },
  { name: "All files", extensions: ["*"] },
];

/**
 * Native multi-image selection for conversions. Returns absolute paths, or
 * null when the dialog was dismissed.
 */
export async function pickImageFiles(): Promise<string[] | null> {
  const selection = await open({
    multiple: true,
    title: "Choose images to convert",
    filters: IMAGE_FILTERS,
  });
  if (selection === null) return null;
  return Array.isArray(selection) ? selection : [selection];
}

/**
 * Native save dialog for a single converted image. `defaultName` is the
 * staged output name (e.g. `photo.png`) so the suggested file name matches
 * the actually-scheduled conversion. Null = dismissed.
 */
export async function pickImageDestination(defaultName: string): Promise<string | null> {
  const extension = defaultName.includes(".")
    ? defaultName.slice(defaultName.lastIndexOf(".") + 1).toLowerCase()
    : "";
  return save({
    title: "Save converted image…",
    defaultPath: defaultName,
    filters:
      extension !== ""
        ? [
            { name: `${extension.toUpperCase()} image`, extensions: [extension] },
            { name: "All files", extensions: ["*"] },
          ]
        : [{ name: "All files", extensions: ["*"] }],
  });
}

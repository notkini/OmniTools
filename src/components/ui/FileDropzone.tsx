import { useEffect, useRef, useState, type DragEvent } from "react";
import { Upload } from "lucide-react";
import { pickFiles } from "../../shared/fs/invoke";

interface FileDropzoneProps {
  label: string;
  /** Hint shown under the title, e.g. accepted formats: "JPG · PNG · WebP". */
  formatHint?: string;
  multiple?: boolean;
  /** Receives absolute filesystem paths (hashing happens in Rust). */
  onFiles: (paths: string[]) => void;
  /** Click-to-browse adapter; injectable for tests (default: native dialog). */
  onBrowse?: () => Promise<string[] | null>;
}

/**
 * Drag-and-drop + click-to-browse for the Tauri webview. Selection yields
 * absolute paths via the scoped dialog plugin; drops arrive as real paths
 * through the webview's drag-drop event. Validating nothing by itself —
 * tools own their format rules and per-file errors.
 */
export function FileDropzone({
  label,
  formatHint,
  multiple = false,
  onFiles,
  onBrowse,
}: FileDropzoneProps) {
  const [isOver, setIsOver] = useState(false);
  const busyRef = useRef(false);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;

    const attach = async () => {
      // Webview-native drag events carry absolute paths for dropped files —
      // HTML5 DataTransfer inside a Tauri webview would only give pathless
      // blobs, which the Rust layer cannot hash.
      const webview = await import("@tauri-apps/api/webview");
      const unlistenFn = await webview.getCurrentWebview().onDragDropEvent((event) => {
        if (event.payload.type === "enter" || event.payload.type === "over") {
          setIsOver(true);
        } else if (event.payload.type === "leave") {
          setIsOver(false);
        } else if (event.payload.type === "drop") {
          setIsOver(false);
          if (event.payload.paths.length > 0) {
            onFiles(event.payload.paths);
          }
        }
      });
      if (cancelled) {
        unlistenFn();
      } else {
        unlisten = unlistenFn;
      }
    };
    attach().catch(() => {
      // Non-Tauri environments (plain web dev / tests) simply get no native
      // drag events; the click-to-browse dialog remains available in-app.
    });

    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, [onFiles, multiple]);

  const openDialog = async () => {
    if (busyRef.current) return;
    busyRef.current = true;
    try {
      const paths = await (onBrowse ?? pickFiles)();
      if (paths && paths.length > 0) {
        // The dialog's multiple flag governs batch size; a single-select
        // dialog returns one path either way.
        onFiles(multiple ? paths : paths.slice(0, 1));
      }
    } finally {
      busyRef.current = false;
    }
  };

  // HTML5 handlers remain for keyboard-free mouse flows in non-Tauri dev;
  // inside Tauri, native drag events above win paths and these are inert.
  const handleDrop = (event: DragEvent<HTMLDivElement>) => {
    event.preventDefault();
    setIsOver(false);
  };

  return (
    <div
      className="workspace-dropzone"
      data-over={isOver}
      role="button"
      tabIndex={0}
      aria-label={`${label} — click to browse${multiple ? ", or drop files" : ", or drop a file"}`}
      onClick={() => void openDialog()}
      onKeyDown={(event) => {
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          void openDialog();
        }
      }}
      onDragOver={handleDrop}
      onDragLeave={() => setIsOver(false)}
      onDrop={handleDrop}
    >
      <Upload size={20} strokeWidth={1.75} />
      <span className="workspace-dropzone-title">{label}</span>
      {formatHint ? <span>{formatHint}</span> : null}
    </div>
  );
}

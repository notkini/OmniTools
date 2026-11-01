import { useRef, useState, type DragEvent } from "react";
import { Upload } from "lucide-react";

interface FileDropzoneProps {
  label: string;
  /** Hint shown under the title, e.g. accepted formats: "JPG · PNG · WebP". */
  formatHint?: string;
  multiple?: boolean;
  onFiles: (files: File[]) => void;
}

/**
 * Drag-and-drop + click-to-browse. Validates nothing by itself —
 * tools own their format rules and error messages.
 */
export function FileDropzone({
  label,
  formatHint,
  multiple = false,
  onFiles,
}: FileDropzoneProps) {
  const inputRef = useRef<HTMLInputElement>(null);
  const [isOver, setIsOver] = useState(false);

  const selectFiles = (files: FileList | null) => {
    if (!files) return;
    const selected = Array.from(files);
    if (selected.length > 0) onFiles(selected);
  };

  const handleDrop = (event: DragEvent<HTMLDivElement>) => {
    event.preventDefault();
    setIsOver(false);
    selectFiles(event.dataTransfer.files);
  };

  const handleDragOver = (event: DragEvent<HTMLDivElement>) => {
    event.preventDefault();
    setIsOver(true);
  };

  return (
    <div
      className="workspace-dropzone"
      data-over={isOver}
      role="button"
      tabIndex={0}
      aria-label={`${label} — click to browse${multiple ? ", or drop files" : ", or drop a file"}`}
      onClick={() => inputRef.current?.click()}
      onKeyDown={(event) => {
        if (event.key === "Enter" || event.key === " ") {
          event.preventDefault();
          inputRef.current?.click();
        }
      }}
      onDragOver={handleDragOver}
      onDragLeave={() => setIsOver(false)}
      onDrop={handleDrop}
    >
      <Upload size={20} strokeWidth={1.75} />
      <span className="workspace-dropzone-title">{label}</span>
      {formatHint ? <span>{formatHint}</span> : null}
      <input
        ref={inputRef}
        type="file"
        multiple={multiple}
        hidden
        tabIndex={-1}
        aria-hidden="true"
        onChange={(event) => {
          selectFiles(event.target.files);
          event.target.value = "";
        }}
      />
    </div>
  );
}

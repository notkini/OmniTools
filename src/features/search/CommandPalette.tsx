import {
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
} from "react";
import { searchTools } from "./matcher";
import { Kbd } from "../../components/ui/Kbd";

interface CommandPaletteProps {
  open: boolean;
  onClose: () => void;
  /** Receives a route path (e.g. a tool's routePath) to activate. */
  onSelect: (path: string) => void;
}

/**
 * Universal search (PLAN §4.1): the spine of keyboard navigation.
 * ARIA combobox + listbox pattern; input keeps focus, selection is exposed
 * via aria-activedescendant so screen readers follow arrow keys.
 */
export function CommandPalette({ open, onClose, onSelect }: CommandPaletteProps) {
  const [query, setQuery] = useState("");
  const [selected, setSelected] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);

  const results = useMemo(() => searchTools(query), [query]);

  // Reset state each time the palette opens.
  useEffect(() => {
    if (open) {
      setQuery("");
      setSelected(0);
    }
  }, [open]);

  // Focus management: focus the input on open; restore focus on close.
  useEffect(() => {
    if (!open) return;
    const previous = document.activeElement;
    inputRef.current?.focus();
    return () => {
      if (previous instanceof HTMLElement) previous.focus();
    };
  }, [open]);

  const resultsRef = useRef<HTMLUListElement>(null);
  useEffect(() => {
    const selectedElement = resultsRef.current?.children[selected];
    if (selectedElement instanceof HTMLElement) {
      // Guarded: jsdom (tests) has no layout engine.
      selectedElement.scrollIntoView?.({ block: "nearest" });
    }
  }, [selected, results]);

  const activate = useCallback(
    (index: number) => {
      const tool = results[index];
      if (!tool) return;
      onSelect(tool.routePath);
      onClose();
    },
    [results, onSelect, onClose],
  );

  const handleKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    switch (event.key) {
      case "Escape":
        event.preventDefault();
        onClose();
        break;
      case "Enter": {
        event.preventDefault();
        activate(selected);
        break;
      }
      case "ArrowDown": {
        event.preventDefault();
        if (results.length > 0) setSelected((i) => (i + 1) % results.length);
        break;
      }
      case "ArrowUp": {
        event.preventDefault();
        if (results.length > 0) {
          setSelected((i) => (i - 1 + results.length) % results.length);
        }
        break;
      }
      case "Home":
        if (results.length > 0) {
          event.preventDefault();
          setSelected(0);
        }
        break;
      case "End":
        if (results.length > 0) {
          event.preventDefault();
          setSelected(results.length - 1);
        }
        break;
      default:
        break;
    }
  };

  if (!open) return null;

  const selectedId =
    results.length > 0 ? `palette-option-${Math.min(selected, results.length - 1)}` : undefined;

  return (
    <div
      className="palette-overlay"
      onClick={onClose}
      data-testid="palette-overlay"
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Command palette"
        className="palette"
        onClick={(event) => event.stopPropagation()}
      >
        <input
          ref={inputRef}
          className="palette-input"
          role="combobox"
          aria-expanded="true"
          aria-controls="palette-results"
          aria-activedescendant={selectedId}
          aria-label="Search tools"
          placeholder="Search tools…"
          value={query}
          onChange={(event) => {
            setQuery(event.target.value);
            setSelected(0);
          }}
          onKeyDown={handleKeyDown}
          spellCheck={false}
        />

        <ul
          id="palette-results"
          ref={resultsRef}
          className="palette-results"
          role="listbox"
          aria-label="Matching tools"
        >
          {results.map((tool, index) => (
            <li
              key={tool.id}
              id={`palette-option-${index}`}
              role="option"
              aria-selected={index === selected}
              className="palette-option"
              data-selected={index === selected}
              onClick={() => activate(index)}
            >
              <tool.icon size={16} strokeWidth={1.75} aria-hidden="true" />
              <span className="palette-option-name">{tool.name}</span>
              <span className="palette-option-description">{tool.description}</span>
            </li>
          ))}
          {results.length === 0 ? (
            <li className="palette-empty" aria-live="polite">
              No matches for “{query.trim()}”
            </li>
          ) : null}
        </ul>

        <div className="palette-footer">
          <span>
            <Kbd>↑</Kbd> <Kbd>↓</Kbd> navigate
          </span>
          <span>
            <Kbd>↵</Kbd> open
          </span>
          <span>
            <Kbd>Esc</Kbd> close
          </span>
        </div>
      </div>
    </div>
  );
}

import { useCallback, type ReactNode } from "react";
import { ArrowDown, ArrowUp, Minus, Plus } from "lucide-react";
import { Button } from "../../../components/ui/Button";
import { Select, TextInput } from "../../../components/ui/Select";
import { IconButton } from "../../../components/ui/IconButton";
import {
  makeOperation,
  makeOperationValue,
  moveOperation,
  operationLabel,
  validateNumbering,
  type CaseMode,
  type OperationType,
  type RenameOperationDraft,
} from "./operations";

type OperationValue = RenameOperationDraft["op"];

const TYPE_OPTIONS: readonly { value: OperationType; label: string }[] = [
  { value: "prefix", label: operationLabel("prefix") },
  { value: "suffix", label: operationLabel("suffix") },
  { value: "replace", label: operationLabel("replace") },
  { value: "numbering", label: operationLabel("numbering") },
  { value: "case", label: operationLabel("case") },
  { value: "extension", label: operationLabel("extension") },
  { value: "pattern", label: operationLabel("pattern") },
];

const CASE_MODE_OPTIONS: readonly { value: CaseMode; label: string }[] = [
  { value: "upper", label: "UPPERCASE" },
  { value: "lower", label: "lowercase" },
  { value: "title", label: "Title Case" },
];

interface OperationBuilderProps {
  operations: RenameOperationDraft[];
  onChange: (operations: RenameOperationDraft[]) => void;
}

/**
 * The stacked operation editor for Batch Rename. It only builds and edits
 * serializable drafts — all actual renaming decisions happen in Rust
 * (`plan_rename`), so nothing here may compute resulting file names.
 */
export function OperationBuilder({ operations, onChange }: OperationBuilderProps) {
  const addOperation = useCallback(() => {
    onChange([...operations, makeOperation("prefix")]);
  }, [operations, onChange]);

  const removeOperation = useCallback(
    (id: string) => {
      onChange(operations.filter((draft) => draft.id !== id));
    },
    [operations, onChange],
  );

  const moveUp = useCallback(
    (id: string) => {
      const index = operations.findIndex((draft) => draft.id === id);
      if (index <= 0) return;
      onChange(moveOperation(operations, index, index - 1));
    },
    [operations, onChange],
  );

  const moveDown = useCallback(
    (id: string) => {
      const index = operations.findIndex((draft) => draft.id === id);
      if (index < 0 || index >= operations.length - 1) return;
      onChange(moveOperation(operations, index, index + 1));
    },
    [operations, onChange],
  );

  const updateValue = useCallback(
    (id: string, value: OperationValue) => {
      onChange(
        operations.map((draft) =>
          draft.id === id ? { ...draft, op: value } : draft,
        ),
      );
    },
    [operations, onChange],
  );

  const operationNodes: ReactNode[] = operations.map((draft, index) => (
    <div
      key={draft.id}
      className="operation-card"
      data-testid={`operation-card-${index + 1}`}
    >
      <div className="operation-card-header">
        <span className="operation-badge" aria-hidden="true">
          {index + 1}
        </span>
        <Select
          label="Operation type"
          value={draft.op.type}
          options={TYPE_OPTIONS}
          onChange={(type) => updateValue(draft.id, makeOperationValue(type))}
        />
        <div className="operation-card-actions">
          <IconButton
            className="row-copy-button"
            label={`Move ${operationLabel(draft.op.type)} up`}
            disabled={index === 0}
            onClick={() => moveUp(draft.id)}
          >
            <ArrowUp size={14} strokeWidth={1.75} aria-hidden="true" />
          </IconButton>
          <IconButton
            className="row-copy-button"
            label={`Move ${operationLabel(draft.op.type)} down`}
            disabled={index === operations.length - 1}
            onClick={() => moveDown(draft.id)}
          >
            <ArrowDown size={14} strokeWidth={1.75} aria-hidden="true" />
          </IconButton>
          <IconButton
            className="row-copy-button"
            label={`Remove ${operationLabel(draft.op.type)}`}
            onClick={() => removeOperation(draft.id)}
          >
            <Minus size={14} strokeWidth={1.75} aria-hidden="true" />
          </IconButton>
        </div>
      </div>
      <div className="operation-card-fields">
        <OperationFields
          value={draft.op}
          onChange={(next) => updateValue(draft.id, next)}
        />
      </div>
    </div>
  ));

  return (
    <div className="operation-stack">
      {operations.length === 0 ? (
        <div className="operation-stack-empty">
          <span className="operation-stack-empty-label">
            No operations — file names stay unchanged
          </span>
        </div>
      ) : (
        operationNodes
      )}
      <div className="operation-stack-add">
        <Button size="sm" onClick={addOperation}>
          <Plus size={16} strokeWidth={1.75} aria-hidden="true" />
          Add operation
        </Button>
      </div>
    </div>
  );
}

interface OperationFieldsProps {
  value: OperationValue;
  onChange: (next: OperationValue) => void;
}

/** Per-type field editors; exhaustive over the operation union. */
function OperationFields({ value, onChange }: OperationFieldsProps) {
  switch (value.type) {
    case "prefix":
      return (
        <div className="operation-field">
          <TextInput
            label="Prefix text"
            value={value.value}
            placeholder="e.g. holiday_"
            onChange={(next) => onChange({ ...value, value: next })}
          />
          <span className="operation-hint">
            Added to the front of each filename.
          </span>
        </div>
      );
    case "suffix":
      return (
        <div className="operation-field">
          <TextInput
            label="Suffix text"
            value={value.value}
            placeholder="e.g. _edited"
            onChange={(next) => onChange({ ...value, value: next })}
          />
          <span className="operation-hint">
            Added before the file extension.
          </span>
        </div>
      );
    case "replace":
      return (
        <div className="operation-fields-row">
          <TextInput
            label="Find"
            value={value.find}
            placeholder="text to replace"
            onChange={(find) => onChange({ ...value, find })}
          />
          <TextInput
            label="Replace with"
            value={value.replaceWith}
            placeholder="replacement text"
            onChange={(replaceWith) => onChange({ ...value, replaceWith })}
          />
          <label className="operation-check">
            <input
              type="checkbox"
              checked={value.caseSensitive}
              onChange={(event) =>
                onChange({ ...value, caseSensitive: event.target.checked })
              }
            />
            <span>Case sensitive</span>
          </label>
        </div>
      );
    case "numbering": {
      const setNumbering = (
        field: "start" | "step" | "padding",
        raw: string,
      ): OperationValue => {
        const next = validateNumbering(
          field === "start" ? Number(raw) : value.start,
          field === "step" ? Number(raw) : value.step,
          field === "padding" ? Number(raw) : value.padding,
        );
        return { ...value, ...next };
      };
      return (
        <div className="operation-fields-row">
          <label className="operation-field">
            <span className="operation-field-label">Start at</span>
            <input
              className="input"
              type="number"
              min={1}
              value={value.start}
              onChange={(event) => onChange(setNumbering("start", event.target.value))}
            />
          </label>
          <label className="operation-field">
            <span className="operation-field-label">Step</span>
            <input
              className="input"
              type="number"
              min={1}
              value={value.step}
              onChange={(event) => onChange(setNumbering("step", event.target.value))}
            />
          </label>
          <label className="operation-field">
            <span className="operation-field-label">Padding width</span>
            <input
              className="input"
              type="number"
              min={0}
              max={20}
              value={value.padding}
              onChange={(event) =>
                onChange(setNumbering("padding", event.target.value))
              }
            />
            <span className="operation-hint">0 = no padding</span>
          </label>
        </div>
      );
    }
    case "case":
      return (
        <Select
          label="Case mode"
          value={value.mode}
          options={CASE_MODE_OPTIONS}
          onChange={(mode) => onChange({ ...value, mode })}
        />
      );
    case "extension":
      return (
        <div className="operation-field">
          <TextInput
            label="Extension"
            value={value.value}
            placeholder="e.g. png"
            onChange={(next) => onChange({ ...value, value: next })}
          />
          <span className="operation-hint">
            Without the leading dot. Leave empty to remove the extension.
          </span>
        </div>
      );
    case "pattern":
      return (
        <div className="operation-field">
          <TextInput
            label="Pattern template"
            value={value.template}
            placeholder="photo_{n}"
            onChange={(template) => onChange({ ...value, template })}
          />
          <span className="operation-hint">
            {"{n}"} sequence number · {"{orig}"} original stem · {"{date}"} today
          </span>
        </div>
      );
  }
}

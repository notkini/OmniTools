import { useCallback, useState } from "react";
import { ChevronDown, ChevronUp, Minus, Plus, X } from "lucide-react";
import { Button } from "../../../components/ui/Button";
import { Select } from "../../../components/ui/Select";
import { IconButton } from "../../../components/ui/IconButton";
import {
  makeOperation,
  makeOperationId,
  moveOperation,
  operationLabel,
  validateNumbering,
  type RenameOperationDraft,
} from "./operations";

const TYPE_OPTIONS: { value: string; label: string }[] = [
  { value: "prefix", label: operationLabel("prefix") },
  { value: "suffix", label: operationLabel("suffix") },
  { value: "replace", label: operationLabel("replace") },
  { value: "numbering", label: operationLabel("numbering") },
  { value: "case", label: operationLabel("case") },
  { value: "extension", label: operationLabel("extension") },
  { value: "pattern", label: operationLabel("pattern") },
];

const CASE_MODES = [
  { value: "upper", label: "UPPERCASE" },
  { value: "lower", label: "lowercase" },
  { value: "title", label: "Title Case" },
];

interface OperationBuilderProps {
  operations: RenameOperationDraft[];
  onChange: (operations: RenameOperationDraft[]) => void;
}

export function OperationBuilder({
  operations,
  onChange,
}: OperationBuilderProps) {
  const [editingId, setEditingId] = useState<string | null>(null);

  const addOperation = useCallback(() => {
    setEditingId(makeOperationId());
    onChange((prev) => [...prev, makeOperation("prefix")]);
  }, [onChange]);

  const removeOperation = useCallback(
    (id: string) => {
      onChange((prev) => prev.filter((op) => op.id !== id));
      if (editingId === id) setEditingId(null);
    },
    [editingId, onChange],
  );

  const moveOperationUp = useCallback(
    (id: string) => {
      const idx = operations.findIndex((op) => op.id === id);
      if (idx <= 0) return;
      onChange((prev: any) => moveOperation(prev, idx, idx - 1));
    },
    [operations, onChange],
  );

  const moveOperationDown = useCallback(
    (id: string) => {
      const idx = operations.findIndex((op) => op.id === id);
      if (idx < 0 || idx >= operations.length - 1) return;
      onChange((prev: any) => moveOperation(prev, idx, idx + 1));
    },
    [operations, onChange],
  );

  const updateOperation = useCallback(
    (id: string, op: any) => {
      onChange((prev: any) =>
        prev.map((o: any) => (o.id === id ? { ...o, op } : o)),
      );
    },
    [onChange],
  );

  if (operations.length === 0) {
    return (
      <div className="operation-stack">
        <div className="operation-stack-empty">
          <span className="operation-stack-empty-label">
            No operations yet — names stay unchanged
          </span>
          <Button
            size="sm"
            onClick={addOperation}
            icon={<Plus size={16} strokeWidth={1.75} aria-hidden="true" />}
          >
            Add operation
          </Button>
        </div>
      </div>
    );
  }

  return (
    <div className="operation-stack">
      {operations.map((op, index) => {
        const isEditing = editingId === op.id;
        return (
          <div
            key={op.id}
            className="operation-card"
            data-testid={`operation-card-${op.id}`}
          >
            <div className="operation-card-header">
              <span className="operation-badge">{index + 1}</span>
              <Select
                label="Operation type"
                value={op.op.type}
                options={TYPE_OPTIONS}
                onChange={(value) => {
                  updateOperation(id, {
                    type: value as RenameOperationDraft["op"]["type"],
                    ...defaultValueForType(
                      value as RenameOperationDraft["op"]["type"],
                    ),
                  });
                }}
                size="sm"
              />
              <div className="operation-card-actions">
                <IconButton
                  label={`Move ${operationLabel(op.op.type)} up`}
                  icon={<ChevronUp size={14} strokeWidth={1.75} aria-hidden="true" />}
                  onClick={() => void moveOperationUp(op.id)}
                  disabled={index === 0}
                />
                <IconButton
                  label={`Move ${operationLabel(op.op.type)} down`}
                  icon={<ChevronDown size={14} strokeWidth={1.75} aria-hidden="true" />}
                  onClick={() => void moveOperationDown(op.id)}
                  disabled={index === operations.length - 1}
                />
                <IconButton
                  label={`Remove ${operationLabel(op.op.type)}`}
                  icon={<Minus size={14} strokeWidth={1.75} aria-hidden="true" />}
                  onClick={() => void removeOperation(op.id)}
                />
              </div>
            </div>
            <div className="operation-card-fields">
              {renderOperationFields(op, isEditing, updateOperation)}
            </div>
          </div>
        );
      })}
      <div className="operation-stack-add">
        <Button
          size="sm"
          onClick={addOperation}
          icon={<Plus size={16} strokeWidth={1.75} aria-hidden="true" />}
        >
          Add operation
        </Button>
      </div>
    </div>
  );
}

function defaultValueForType(
  type: RenameOperationDraft["op"]["type"],
): RenameOperationDraft["op"] {
  switch (type) {
    case "prefix":
      return { type: "prefix", value: "" };
    case "suffix":
      return { type: "suffix", value: "" };
    case "replace":
      return { type: "replace", find: "", replaceWith: "", caseSensitive: false };
    case "numbering":
      return { type: "numbering", start: 1, step: 1, padding: 1 };
    case "case":
      return { type: "case", mode: "upper" };
    case "extension":
      return { type: "extension", value: "" };
    case "pattern":
      return { type: "pattern", template: "" };
  }
}

function renderOperationFields(
  op: RenameOperationDraft,
  _isEditing: boolean,
  onChange: (id: string, op: any) => void,
): JSX.Element {
  switch (op.op.type) {
    case "prefix":
      return (
        <div className="operation-field">
          <label className="operation-field-label">Prefix text</label>
          <input
            type="text"
            value={op.op.value}
            onChange={(e) =>
              void onChange(op.id, { ...op.op, value: e.target.value })}
            placeholder="e.g. holiday_"
          />
          <span className="operation-hint">
            Added to the front of each filename.
          </span>
        </div>
      );
    case "suffix":
      return (
        <div className="operation-field">
          <label className="operation-field-label">Suffix text</label>
          <input
            type="text"
            value={op.op.value}
            onChange={(e) => {
              void onChange(op.id, { ...op.op, value: e.target.value });
            }}
            placeholder="e.g. _edited"
          />
          <span className="operation-hint">
            Added before the file extension.
          </span>
        </div>
      );
    case "replace":
      return (
        <div className="operation-fields-row">
          <div className="operation-field">
            <label className="operation-field-label">Find</label>
            <input
              type="text"
              value={op.op.find}
              onChange={(e) => {
                void onChange(op.id, { ...op.op, find: e.target.value });
              }}
              placeholder="text to replace"
            />
          </div>
          <div className="operation-field">
            <label className="operation-field-label">Replace with</label>
            <input
              type="text"
              value={op.op.replaceWith}
              onChange={(e) => {
                void onChange(op.id, { ...op.op, replaceWith: e.target.value });
              }}
              placeholder="replacement text"
            />
          </div>
          <div className="operation-field">
            <label className="operation-field-label">Case sensitive</label>
            <input
              type="checkbox"
              checked={op.op.caseSensitive}
              onChange={(e) => {
                void onChange(op.id, {
                  ...op.op,
                  caseSensitive: e.target.checked,
                });
              }}
            />
          </div>
        </div>
      );
    case "numbering":
      return (
        <div className="operation-fields-row">
          <div className="operation-field">
            <label className="operation-field-label">Start at</label>
            <input
              type="number"
              min="0"
              value={op.op.start.toString()}
              onChange={(e) => {
                void onChange(op.id, {
                  ...op.op,
                  start: validateNumbering(
                    Number(e.target.value),
                    op.op.step,
                    op.op.padding,
                  ).start,
                });
              }}
            />
          </div>
          <div className="operation-field">
            <label className="operation-field-label">Step</label>
            <input
              type="number"
              min="0"
              value={op.op.step.toString()}
              onChange={(e) => {
                void onChange(op.id, {
                  ...op.op,
                  step: validateNumbering(
                    op.op.start,
                    Number(e.target.value),
                    op.op.padding,
                  ).step,
                });
              }}
            />
          </div>
          <div className="operation-field">
            <label className="operation-field-label">Padding width</label>
            <input
              type="number"
              min="0"
              max="20"
              value={op.op.padding.toString()}
              onChange={(e) => {
                void onChange(op.id, {
                  ...op.op,
                  padding: validateNumbering(
                    op.op.start,
                    op.op.step,
                    Number(e.target.value),
                  ).padding,
                });
              }}
            />
            <span className="operation-hint">0 = no padding</span>
          </div>
        </div>
      );
    case "case":
      return (
        <div className="operation-field">
          <label className="operation-field-label">Case mode</label>
          <Select
            value={op.op.mode}
            options={CASE_MODES.map((m) => ({ value: m.value, label: m.label }))}
            onChange={(value) => {
              void onChange(op.id, {
                ...op.op,
                mode: value as "upper" | "lower" | "title",
              });
            }}
            size="sm"
          />
        </div>
      );
    case "extension":
      return (
        <div className="operation-field">
          <label className="operation-field-label">Extension</label>
          <input
            type="text"
            value={op.op.value}
            onChange={(e) => void onChange(op.id, { ...op.op, value: e.target.value })}
            placeholder="e.g. png"
          />
          <span className="operation-hint">
            Without the leading dot. Leave empty to remove extension.
          </span>
        </div>
      );
    case "pattern":
      return (
        <div className="operation-field">
          <label className="operation-field-label">Pattern template</label>
          <input
            type="text"
            value={op.op.template}
            onChange={(e) => void onChange(op.id, { ...op.op, template: e.target.value })}
            placeholder="{n} - {orig} - {date}"
          />
          <span className="operation-hint">
            {n} = sequence number · {orig} = original name · {date} = today's date
          </span>
        </div>
      );
  }
}

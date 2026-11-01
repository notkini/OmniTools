import type { ReactNode } from "react";

interface SelectProps<Value extends string> {
  label: string;
  value: Value;
  options: readonly { value: Value; label: ReactNode }[];
  onChange: (value: Value) => void;
  hint?: string;
}

/** Native select, minimally styled. The label is visible, not placeholder-empty. */
export function Select<Value extends string>({
  label,
  value,
  options,
  onChange,
  hint,
}: SelectProps<Value>) {
  return (
    <label className="field">
      <span className="field-label">{label}</span>
      <span>
        <select
          className="select"
          value={value}
          onChange={(event) => {
            onChange(event.target.value as Value);
          }}
        >
          {options.map((option) => (
            <option key={option.value} value={option.value}>
              {option.label}
            </option>
          ))}
        </select>
      </span>
      {hint ? <span className="field-hint">{hint}</span> : null}
    </label>
  );
}

interface TextInputProps {
  label: string;
  value: string;
  placeholder?: string;
  onChange: (value: string) => void;
}

export function TextInput({
  label,
  value,
  placeholder,
  onChange,
  id,
}: TextInputProps & { id?: string }) {
  const inputId = id ?? `input-${label.toLowerCase().replace(/\s+/g, "-")}`;
  return (
    <label className="field" htmlFor={inputId}>
      <span className="field-label">{label}</span>
      <input
        id={inputId}
        className="input"
        type="text"
        value={value}
        placeholder={placeholder}
        onChange={(event) => {
          onChange(event.target.value);
        }}
      />
    </label>
  );
}

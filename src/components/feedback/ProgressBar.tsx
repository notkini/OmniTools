interface ProgressBarProps {
  /** 0–100. Omit for indeterminate progress. */
  value?: number;
  label: string;
}

export function ProgressBar({ value, label }: ProgressBarProps) {
  if (value === undefined) {
    return (
      <div className="progress progress--indeterminate" role="progressbar" aria-busy="true" aria-label={label}>
        <div className="progress-bar" />
      </div>
    );
  }
  const clamped = Math.max(0, Math.min(100, value));
  return (
    <div
      className="progress"
      role="progressbar"
      aria-busy={clamped < 100}
      aria-valuenow={clamped}
      aria-valuemin={0}
      aria-valuemax={100}
      aria-label={label}
    >
      <div className="progress-bar" style={{ width: `${clamped}%` }} />
    </div>
  );
}

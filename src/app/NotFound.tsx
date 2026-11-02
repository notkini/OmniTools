import { Button } from "../components/ui/Button";

interface NotFoundProps {
  /** The path that did not resolve — shown honestly, never hidden. */
  path: string;
  onGoHome: () => void;
}

export function NotFound({ path, onGoHome }: NotFoundProps) {
  return (
    <div className="view-content">
      <div className="not-found">
        <h1 className="page-title">Page not found</h1>
        <p className="field-hint">
          No page lives at <code>{path}</code>. It may have been renamed or is
          mistyped.
        </p>
        <Button onClick={onGoHome}>Go to Home</Button>
      </div>
    </div>
  );
}

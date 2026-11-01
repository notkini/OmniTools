import { Monitor, Moon, Sun } from "lucide-react";
import { useTheme, nextThemePreference } from "./ThemeProvider";
import { IconButton } from "../components/ui/IconButton";
import { Tooltip } from "../components/ui/Tooltip";
import type { ResolvedTheme, ThemePreference } from "./theme";

const PREFERENCE_LABEL: Record<ThemePreference, string> = {
  light: "Light",
  dark: "Dark",
  system: "System",
};

interface TopBarProps {
  title: string;
}

export function TopBar({ title }: TopBarProps) {
  const { preference, resolved, setPreference } = useTheme();

  return (
    <header className="topbar">
      <span className="topbar-title">{title}</span>
      <div className="topbar-actions">
        <span className="topbar-theme-label">
          {PREFERENCE_LABEL[preference]}
        </span>
        <Tooltip tip={`Theme: ${PREFERENCE_LABEL[preference]}`}>
          <IconButton
            label={`Switch theme to ${nextThemePreference(preference)}`}
            onClick={() => setPreference(nextThemePreference(preference))}
          >
            <ThemeIcon resolved={resolved} preference={preference} />
          </IconButton>
        </Tooltip>
      </div>
    </header>
  );
}

function ThemeIcon({
  resolved,
  preference,
}: {
  resolved: ResolvedTheme;
  preference: ThemePreference;
}) {
  if (preference === "system") {
    return <Monitor size={16} strokeWidth={1.75} />;
  }
  return resolved === "dark" ? (
    <Moon size={16} strokeWidth={1.75} />
  ) : (
    <Sun size={16} strokeWidth={1.75} />
  );
}

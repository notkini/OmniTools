import { useTheme } from "./ThemeProvider";
import { Select } from "../components/ui/Select";
import type { ThemePreference } from "./theme";

const THEME_OPTIONS: {
  value: ThemePreference;
  label: string;
}[] = [
  { value: "light", label: "Light" },
  { value: "dark", label: "Dark" },
  { value: "system", label: "Follow system" },
];

/** Settings shell screen. Only settings that exist and work appear here. */
export function Settings() {
  const { preference, setPreference } = useTheme();

  return (
    <div className="view-content">
      <div className="settings-sections">
        <section className="settings-section">
          <h2 className="section-title">Appearance</h2>
          <Select<ThemePreference>
            label="Theme"
            value={preference}
            options={THEME_OPTIONS}
            onChange={setPreference}
            hint="Applies immediately and is remembered between sessions."
          />
        </section>

        <section className="settings-section">
          <h2 className="section-title">About</h2>
          <dl className="kv">
            <dt>Version</dt>
            <dd>0.1.0 (Milestone 2)</dd>
            <dt>Licence</dt>
            <dd>Apache-2.0</dd>
            <dt>Privacy</dt>
            <dd>
              OmniTools performs no network requests and works without an
              account. Files are processed locally.
            </dd>
          </dl>
        </section>
      </div>
    </div>
  );
}

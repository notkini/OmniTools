import { TOOLS } from "../features/tool-registry/registry";
import { APP_VERSION } from "../shared/constants/appInfo";

interface HomeProps {
  onNavigate: (path: string) => void;
}

/**
 * Home — hero plus the real tool list, rendered from the registry.
 * No fake affordances: the list grows exactly as tools register.
 */
export function Home({ onNavigate }: HomeProps) {
  return (
    <div className="view-content">
      <section className="home-hero">
        <h1>OmniTools</h1>
        <p className="lede">
          Everyday file utilities that run entirely on your device.
        </p>
        <div className="facts">
          <span>Version {APP_VERSION}</span>
          <span>Free &amp; open source</span>
          <span>No accounts</span>
          <span>Works offline</span>
        </div>
      </section>

      <section className="home-tools" aria-label="Tools">
        <h2 className="home-tools-heading">Tools</h2>
        <div className="home-tools-list">
          {TOOLS.map((tool) => (
            <button
              key={tool.id}
              type="button"
              className="tool-row"
              onClick={() => onNavigate(tool.routePath)}
            >
              <tool.icon size={18} strokeWidth={1.75} aria-hidden="true" />
              <span className="tool-row-text">
                <span className="tool-row-name">{tool.name}</span>
                <span className="tool-row-description">{tool.description}</span>
              </span>
            </button>
          ))}
        </div>
      </section>
    </div>
  );
}

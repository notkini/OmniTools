import type { LucideIcon } from "lucide-react";
import { House, PanelLeftClose, PanelLeftOpen, Settings } from "lucide-react";
import { TOOLS } from "../features/tool-registry/registry";

/**
 * Navigation truth: shell entries (Home, Settings) are declared here; tool
 * entries come from the registry — a new tool appears in the sidebar with
 * zero edits to this component.
 */
interface ShellNavEntry {
  path: string;
  label: string;
  icon: LucideIcon;
}

const SHELL_NAV_ENTRIES: ShellNavEntry[] = [
  { path: "/", label: "Home", icon: House },
  { path: "/settings", label: "Settings", icon: Settings },
];

interface SidebarProps {
  /** Current route path (e.g. "/", "/settings", "#/tools/about"). */
  activePath: string;
  collapsed: boolean;
  onNavigate: (path: string) => void;
  onCollapseToggle: () => void;
}

export function Sidebar({
  activePath,
  collapsed,
  onNavigate,
  onCollapseToggle,
}: SidebarProps) {
  const CollapseIcon = collapsed ? PanelLeftOpen : PanelLeftClose;

  return (
    <aside className="sidebar" data-collapsed={collapsed}>
      <button
        type="button"
        className="sidebar-brand"
        onClick={() => onNavigate("/")}
        aria-label="OmniTools — go to Home"
      >
        <img src="/logo.svg" alt="" aria-hidden="true" />
        <span className="wordmark">OmniTools</span>
      </button>

      <nav className="sidebar-nav" aria-label="Primary">
        {SHELL_NAV_ENTRIES.map(({ path, label, icon: Icon }) => (
          <button
            key={path}
            type="button"
            className="nav-item"
            aria-current={activePath === path ? "page" : undefined}
            onClick={() => onNavigate(path)}
          >
            <Icon size={16} strokeWidth={1.75} />
            <span className="nav-label">{label}</span>
          </button>
        ))}

        <p className="nav-caption" id="sidebar-tools-caption">
          Tools
        </p>
        <div
          className="sidebar-tools"
          role="group"
          aria-labelledby="sidebar-tools-caption"
        >
          {TOOLS.map((tool) => (
            <button
              key={tool.id}
              type="button"
              className="nav-item"
              aria-current={activePath === tool.routePath ? "page" : undefined}
              onClick={() => onNavigate(tool.routePath)}
            >
              <tool.icon size={16} strokeWidth={1.75} />
              <span className="nav-label">{tool.name}</span>
            </button>
          ))}
        </div>
      </nav>

      <div className="sidebar-footer">
        <button
          type="button"
          className="collapse-button"
          aria-label={collapsed ? "Expand sidebar" : "Collapse sidebar"}
          onClick={onCollapseToggle}
        >
          <CollapseIcon size={16} strokeWidth={1.75} />
          <span className="nav-label">Collapse</span>
        </button>
      </div>
    </aside>
  );
}

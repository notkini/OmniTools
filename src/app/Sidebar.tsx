import type { LucideIcon } from "lucide-react";
import { House, PanelLeftClose, PanelLeftOpen, Settings } from "lucide-react";

export type ViewId = "home" | "settings";

interface NavEntry {
  id: ViewId;
  label: string;
  icon: LucideIcon;
}

/**
 * M2 navigation truth: Home + Settings only. Tool entries are driven by the
 * tool registry which lands in Milestone 3 — the nav list consumes registry
 * data then, without redesigning this component.
 */
const NAV_ENTRIES: NavEntry[] = [
  { id: "home", label: "Home", icon: House },
  { id: "settings", label: "Settings", icon: Settings },
];

interface SidebarProps {
  activeView: ViewId;
  collapsed: boolean;
  onViewSelect: (view: ViewId) => void;
  onCollapseToggle: () => void;
}

export function Sidebar({
  activeView,
  collapsed,
  onViewSelect,
  onCollapseToggle,
}: SidebarProps) {
  const CollapseIcon = collapsed ? PanelLeftOpen : PanelLeftClose;

  return (
    <aside className="sidebar" data-collapsed={collapsed}>
      <div className="sidebar-brand">
        <img src="/logo.svg" alt="" aria-hidden="true" />
        <span className="wordmark">OmniTools</span>
      </div>

      <nav className="sidebar-nav" aria-label="Primary">
        {NAV_ENTRIES.map(({ id, label, icon: Icon }) => (
          <button
            key={id}
            type="button"
            className="nav-item"
            aria-current={activeView === id ? "page" : undefined}
            onClick={() => onViewSelect(id)}
          >
            <Icon size={16} strokeWidth={1.75} />
            <span className="nav-label">{label}</span>
          </button>
        ))}
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

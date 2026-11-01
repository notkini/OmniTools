import { useState } from "react";
import { Sidebar, type ViewId } from "./Sidebar";
import { TopBar } from "./TopBar";
import { Home } from "./Home";
import { Settings } from "./Settings";

/**
 * Application shell (Milestone 2): fixed collapsible sidebar, top bar, and a
 * scroll-owned main region. Views are local state until the tool registry
 * and router replace them in Milestone 3.
 */
export default function App() {
  const [view, setView] = useState<ViewId>("home");
  const [collapsed, setCollapsed] = useState(() => {
    try {
      return localStorage.getItem("omnitools.sidebarCollapsed") === "true";
    } catch {
      return false;
    }
  });

  const toggleCollapse = () => {
    const next = !collapsed;
    setCollapsed(next);
    try {
      localStorage.setItem("omnitools.sidebarCollapsed", String(next));
    } catch {
      // Best-effort persistence only.
    }
  };

  return (
    <div className="shell">
      <a className="skip-link" href="#main-view">
        Skip to content
      </a>

      <Sidebar
        activeView={view}
        collapsed={collapsed}
        onViewSelect={setView}
        onCollapseToggle={toggleCollapse}
      />

      <div className="main-area">
        <TopBar title={view === "home" ? "Home" : "Settings"} />
        <main id="main-view" className="view">
          {view === "home" ? <Home /> : <Settings />}
        </main>
      </div>
    </div>
  );
}

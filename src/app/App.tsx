import { useEffect, useState, type ComponentType } from "react";
import { Sidebar } from "./Sidebar";
import { TopBar } from "./TopBar";
import { Home } from "./Home";
import { Settings } from "./Settings";
import { NotFound } from "./NotFound";
import { routeTitle, useRoute } from "./router";
import { getTool } from "../features/tool-registry/registry";
import type { ToolDefinition } from "../features/tool-registry/types";
import { CommandPalette } from "../features/search/CommandPalette";

const COLLAPSE_KEY = "omnitools.sidebarCollapsed";

/**
 * Application shell: registry-driven navigation, hash routing, and the
 * command palette. Tool pages are lazy-loaded from their registry loaders.
 */
export default function App() {
  const [route, navigate] = useRoute(getTool);
  const [collapsed, setCollapsed] = useState(() => {
    try {
      return localStorage.getItem(COLLAPSE_KEY) === "true";
    } catch {
      return false;
    }
  });
  const [paletteOpen, setPaletteOpen] = useState(false);

  // Command palette toggle: Ctrl+K or Ctrl+P (global, works from any view).
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && (event.key === "k" || event.key === "p")) {
        event.preventDefault();
        setPaletteOpen((open) => !open);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  const toggleCollapse = () => {
    const next = !collapsed;
    setCollapsed(next);
    try {
      localStorage.setItem(COLLAPSE_KEY, String(next));
    } catch {
      // Best-effort persistence only.
    }
  };

  const activePath = routePath(route);

  return (
    <div className="shell">
      <a className="skip-link" href="#main-view">
        Skip to content
      </a>

      <Sidebar
        activePath={activePath}
        collapsed={collapsed}
        onNavigate={navigate}
        onCollapseToggle={toggleCollapse}
      />

      <div className="main-area">
        <TopBar title={routeTitle(route)} />
        <main id="main-view" className="view">
          {route.kind === "home" ? <Home onNavigate={navigate} /> : null}
          {route.kind === "settings" ? <Settings /> : null}
          {route.kind === "tool" ? <ToolView key={route.tool.id} tool={route.tool} /> : null}
          {route.kind === "notFound" ? <NotFound path={route.path} onGoHome={() => navigate("/")} /> : null}
        </main>
      </div>

      <CommandPalette
        open={paletteOpen}
        onClose={() => setPaletteOpen(false)}
        onSelect={(path) => {
          navigate(path);
          setPaletteOpen(false);
        }}
      />
    </div>
  );
}

/** Identifies the route for sidebar aria-current (notFound has no match). */
function routePath(route: ReturnType<typeof useRoute>[0]): string {
  switch (route.kind) {
    case "home":
      return "/";
    case "settings":
      return "/settings";
    case "tool":
      return route.tool.routePath;
    case "notFound":
      return route.path;
  }
}

/**
 * Lazily materialises a tool page from its registry loader. The loader is
 * the real async boundary — this brief "Loading" state reflects actual work.
 */
function ToolView({ tool }: { tool: ToolDefinition }) {
  const [Component, setComponent] = useState<ComponentType | null>(null);
  const [failed, setFailed] = useState(false);

  useEffect(() => {
    let cancelled = false;
    setComponent(null);
    setFailed(false);
    tool
      .load()
      .then((module) => {
        if (!cancelled) setComponent(() => module.default);
      })
      .catch(() => {
        if (!cancelled) setFailed(true);
      });
    return () => {
      cancelled = true;
    };
  }, [tool]);

  if (failed) {
    return (
      <div className="view-content">
        <p className="field-hint" role="alert">
          This tool failed to load. Restart the app and try again.
        </p>
      </div>
    );
  }
  if (!Component) {
    return (
      <div className="view-content">
        <p className="field-hint">Loading…</p>
      </div>
    );
  }
  return <Component />;
}

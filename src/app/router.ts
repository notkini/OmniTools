import { useCallback, useEffect, useState } from "react";
import type { ToolDefinition, ToolRoutePath } from "../features/tool-registry/types";

/**
 * In-app hash router — no routing dependency (documented in the M3 plan):
 * the app has exactly one flat level of destinations, so a tiny pure parser
 * plus a `hashchange` hook covers deep links, back/forward, and WebView
 * compatibility without infra cost.
 */

export type Route =
  | { kind: "home" }
  | { kind: "settings" }
  | { kind: "tool"; tool: ToolDefinition }
  | { kind: "notFound"; path: string };

export type AppPath = "/" | "/settings" | ToolRoutePath | (string & {});

/** Strip the leading fragment marker and normalise empty hashes. */
export function hashToPath(hash: string): string {
  const raw = hash.startsWith("#") ? hash.slice("#".length) : hash;
  if (raw === "" || raw === "/" || raw === "//") return "/";
  return raw;
}

export function parseHash(
  hash: string,
  getTool: (id: string) => ToolDefinition | undefined,
): Route {
  const path = hashToPath(hash);
  if (path === "/") return { kind: "home" };
  if (path === "/settings") return { kind: "settings" };
  const toolSlug = /^\/tools\/([^/?]+)$/.exec(path)?.[1];
  if (toolSlug) {
    const tool = getTool(toolSlug);
    if (tool) return { kind: "tool", tool };
  }
  return { kind: "notFound", path };
}

export function navigateToPath(path: AppPath | string): void {
  // Exactly one '#' prefix — routePath values are bare paths without '#'.
  window.location.hash = "#" + path;
}

export function useRoute(
  getTool: (id: string) => ToolDefinition | undefined,
): [Route, (path: string) => void] {
  const compute = useCallback(
    (): Route => parseHash(window.location.hash, getTool),
    [getTool],
  );

  const [route, setRoute] = useState<Route>(compute);

  useEffect(() => {
    const onHashChange = () => {
      setRoute(compute());
    };
    // Re-evaluate after mount (hash may have changed before subscription).
    setRoute(compute());
    window.addEventListener("hashchange", onHashChange);
    return () => window.removeEventListener("hashchange", onHashChange);
  }, [compute]);

  const navigate = useCallback((path: string) => {
    navigateToPath(path);
  }, []);

  return [route, navigate];
}

export function routeTitle(route: Route): string {
  switch (route.kind) {
    case "home":
      return "Home";
    case "settings":
      return "Settings";
    case "tool":
      return route.tool.name;
    case "notFound":
      return "Not found";
  }
}

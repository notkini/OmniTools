import type { ReactNode } from "react";

interface TooltipProps {
  tip: string;
  children: ReactNode;
}

/*
 * Minimal tooltip: native `title` at M2 level. A positioned custom tooltip
 * arrives only if/when needs outgrow native behaviour (PLAN: avoid
 * unnecessary abstractions).
 */
export function Tooltip({ tip, children }: TooltipProps) {
  return <span title={tip}>{children}</span>;
}

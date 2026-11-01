import type { ReactNode } from "react";

interface KbdProps {
  children: ReactNode;
}

/** Keyboard hint chip, e.g. <Kbd>Ctrl</Kbd> <Kbd>K</Kbd> */
export function Kbd({ children }: KbdProps) {
  return <kbd className="kbd">{children}</kbd>;
}

// What every page sits in: Mantine with the Vox theme, notifications, and the Void behind.

import "@mantine/core/styles.css";
import "@mantine/notifications/styles.css";
import "../styles/global.css";

import { MantineProvider } from "@mantine/core";
import { Notifications } from "@mantine/notifications";
import { StrictMode, type ReactNode } from "react";
import { createRoot } from "react-dom/client";
import { theme } from "../theme";

export function Void() {
  return (
    <div className="vox-void" aria-hidden="true">
      <div className="moon great" />
      <div className="moon red" />
      <div className="moon cold" />
      <div className="floor" />
    </div>
  );
}

export function Shell({ children, void: showVoid = true }: { children: ReactNode; void?: boolean }) {
  return (
    <MantineProvider theme={theme} forceColorScheme="light">
      <Notifications position="top-center" />
      {showVoid && <Void />}
      {children}
    </MantineProvider>
  );
}

/** Start a page: render `page` into #root inside the shell. */
export function mount(page: ReactNode, { void: showVoid = true } = {}) {
  const root = document.getElementById("root");
  if (!root) throw new Error("no #root on this page");
  createRoot(root).render(
    <StrictMode>
      <Shell void={showVoid}>{page}</Shell>
    </StrictMode>,
  );
}

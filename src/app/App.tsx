import { useEffect } from "react";
import { listen } from "@tauri-apps/api/event";
import type { HistoryEntry } from "../types";
import { Sidebar } from "./Sidebar";
import { ViewRouter } from "./ViewRouter";
import { ViewProvider, useView } from "./ViewContext";

/**
 * Inner shell — lives inside the provider so the single `flow://history`
 * listener can bump `dataVersion`. Every view that derives data from history
 * (Home, Insights) reacts to that counter instead of owning its own listener,
 * so a new dictation refreshes the whole shell from one subscription.
 */
function Shell() {
  const { bumpDataVersion } = useView();

  useEffect(() => {
    const unlisten = listen<HistoryEntry>("flow://history", () => {
      bumpDataVersion();
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [bumpDataVersion]);

  return (
    <div className="flex h-dvh bg-bg text-text">
      <Sidebar />
      <main className="flex-1 overflow-y-auto p-6">
        <div className="mx-auto min-h-full max-w-3xl rounded-[var(--radius)] border border-border bg-surface">
          <ViewRouter />
        </div>
      </main>
    </div>
  );
}

export default function App() {
  return (
    <ViewProvider>
      <Shell />
    </ViewProvider>
  );
}

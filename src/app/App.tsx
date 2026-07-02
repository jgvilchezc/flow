import { useEffect, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import type { HistoryEntry, UpdateInfo } from "../types";
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
  const [update, setUpdate] = useState<UpdateInfo | null>(null);

  useEffect(() => {
    const unlisten = listen<HistoryEntry>("flow://history", () => {
      bumpDataVersion();
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, [bumpDataVersion]);

  // App-level update banner: the startup check emits this event, so a newer
  // release surfaces regardless of the current view. Non-blocking and
  // dismissable — the Settings view offers the same manual check.
  useEffect(() => {
    const unlisten = listen<UpdateInfo>("flow://update-available", (event) => {
      setUpdate(event.payload);
    });
    return () => {
      void unlisten.then((fn) => fn());
    };
  }, []);

  return (
    <div className="flex h-dvh bg-bg text-text">
      <Sidebar />
      <main className="flex-1 overflow-y-auto p-6">
        <div className="mx-auto flex min-h-full max-w-3xl flex-col gap-4">
          {update && (
            <div
              role="status"
              className="flex items-center gap-4 rounded-[var(--radius)] border border-accent/30 bg-accent-soft px-4 py-3 text-[13px] leading-relaxed text-text"
            >
              <span className="flex-1">
                Flow {update.version} is available.
              </span>
              <button
                type="button"
                onClick={() => {
                  void openUrl(update.url);
                }}
                className="shrink-0 rounded-md bg-accent px-3 py-1.5 text-[12.5px] font-medium text-white hover:bg-accent/90"
              >
                Download
              </button>
              <button
                type="button"
                aria-label="Dismiss update notice"
                onClick={() => setUpdate(null)}
                className="shrink-0 cursor-pointer text-[13px] font-medium text-muted outline-none hover:text-text focus-visible:text-text"
              >
                Dismiss
              </button>
            </div>
          )}
          <div className="flex-1 rounded-[var(--radius)] border border-border bg-surface">
            <ViewRouter />
          </div>
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

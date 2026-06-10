import type { ReactNode } from "react";
import { openUrl } from "@tauri-apps/plugin-opener";
import { cn } from "../lib/cn";
import { useView, type View } from "./ViewContext";

const REPO_URL = "https://github.com/jgvilchez/flow";

/** Inline 1.5px-stroke icons — one consistent line-icon family, no icon lib. */
const icons: Record<View | "help", ReactNode> = {
  home: (
    <path d="M3 10.5 12 4l9 6.5M5 9.5V20h14V9.5" />
  ),
  insights: (
    <path d="M4 19V5m0 14h16M8 19v-6m4 6V9m4 10v-8" />
  ),
  dictionary: (
    <path d="M5 4h11a3 3 0 0 1 3 3v13H8a3 3 0 0 1-3-3V4Zm0 0v13m3-9h7M8 12h5" />
  ),
  snippets: (
    <path d="M8 4H6a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-2M9 4h6m-6 0v2a1 1 0 0 0 1 1h4a1 1 0 0 0 1-1V4m1 7-3 3 3 3m4-6 3 3-3 3" />
  ),
  style: (
    <path d="M12 3a9 9 0 1 0 0 18c1.1 0 2-.9 2-2 0-.5-.2-1-.5-1.3-.3-.4-.5-.8-.5-1.2 0-1 .8-1.8 1.8-1.8H17a4 4 0 0 0 4-4c0-3.9-4-6.7-9-6.7Zm-4.5 9a1.2 1.2 0 1 1 0-2.5 1.2 1.2 0 0 1 0 2.5Zm3-4a1.2 1.2 0 1 1 0-2.5 1.2 1.2 0 0 1 0 2.5Zm5 0a1.2 1.2 0 1 1 0-2.5 1.2 1.2 0 0 1 0 2.5Z" />
  ),
  settings: (
    <>
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 13a7.8 7.8 0 0 0 0-2l2-1.5-2-3.5-2.4 1a7.6 7.6 0 0 0-1.7-1L15 0h-6l-.3 2.6a7.6 7.6 0 0 0-1.7 1l-2.4-1-2 3.5L4.6 11a7.8 7.8 0 0 0 0 2l-2 1.5 2 3.5 2.4-1c.5.4 1.1.7 1.7 1L9 24h6l.3-2.6c.6-.3 1.2-.6 1.7-1l2.4 1 2-3.5-2-1.5Z" />
    </>
  ),
  help: <path d="M9.1 9a3 3 0 0 1 5.8 1c0 2-3 3-3 3m0 4h.01M12 21a9 9 0 1 1 0-18 9 9 0 0 1 0 18Z" />,
};

const NAV: ReadonlyArray<{ view: View; label: string }> = [
  { view: "home", label: "Home" },
  { view: "insights", label: "Insights" },
  { view: "dictionary", label: "Dictionary" },
  { view: "snippets", label: "Snippets" },
  { view: "style", label: "Style" },
];

function Icon({ children }: { children: ReactNode }) {
  return (
    <svg
      width="18"
      height="18"
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      aria-hidden="true"
    >
      {children}
    </svg>
  );
}

function NavItem({
  view,
  label,
  active,
  onSelect,
}: {
  view: View | "help";
  label: string;
  active: boolean;
  onSelect: () => void;
}) {
  return (
    <button
      type="button"
      aria-current={active ? "page" : undefined}
      onClick={onSelect}
      className={cn(
        "flex w-full items-center gap-3 rounded-[var(--radius)] px-3 py-2",
        "text-[13.5px] font-medium cursor-pointer outline-none transition-colors duration-150",
        "focus-visible:ring-2 focus-visible:ring-accent/40",
        active
          ? "bg-accent-soft text-accent"
          : "text-muted hover:bg-bg hover:text-text",
      )}
    >
      <Icon>{icons[view]}</Icon>
      <span>{label}</span>
    </button>
  );
}

export function Sidebar() {
  const { view, setView } = useView();

  return (
    <nav
      aria-label="Primary"
      className="flex w-60 shrink-0 flex-col gap-1 border-r border-border bg-surface px-3 py-4"
    >
      <div className="px-3 pb-4">
        <span className="text-lg font-semibold tracking-tight text-text">
          Flow
        </span>
      </div>

      <div className="flex flex-col gap-1">
        {NAV.map((item) => (
          <NavItem
            key={item.view}
            view={item.view}
            label={item.label}
            active={view === item.view}
            onSelect={() => setView(item.view)}
          />
        ))}
      </div>

      <div className="mt-auto flex flex-col gap-1 border-t border-border pt-3">
        <NavItem
          view="settings"
          label="Settings"
          active={view === "settings"}
          onSelect={() => setView("settings")}
        />
        <NavItem
          view="help"
          label="Help"
          active={false}
          onSelect={() => {
            void openUrl(REPO_URL);
          }}
        />
      </div>
    </nav>
  );
}

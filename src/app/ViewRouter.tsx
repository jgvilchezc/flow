import { SettingsView } from "../views/SettingsView";
import { HomeView } from "../views/HomeView";
import { EmptyState } from "../components/ui/EmptyState";
import { useView, type View } from "./ViewContext";

const SparkIcon = (
  <svg
    width="22"
    height="22"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="1.5"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    <path d="M12 3v3m0 12v3M3 12h3m12 0h3M5.6 5.6l2.1 2.1m8.6 8.6 2.1 2.1m0-12.8-2.1 2.1M7.7 16.3l-2.1 2.1" />
  </svg>
);

const PLACEHOLDERS: Record<
  Exclude<View, "settings" | "home">,
  { title: string; hint: string }
> = {
  insights: {
    title: "Insights",
    hint: "Words dictated, words-per-minute, streaks, and per-app usage. Coming in the next batch.",
  },
  dictionary: {
    title: "Dictionary",
    hint: "Teach Flow your proper nouns and literal replacements. Coming in the next batch.",
  },
  snippets: {
    title: "Snippets",
    hint: "Expand short triggers into full phrases as you speak. Coming in the next batch.",
  },
  style: {
    title: "Style",
    hint: "Tune tone per context — personal, work, email, other. Coming in the next batch.",
  },
};

export function ViewRouter() {
  const { view } = useView();

  if (view === "settings") {
    return <SettingsView />;
  }

  if (view === "home") {
    return <HomeView />;
  }

  const placeholder = PLACEHOLDERS[view];
  return (
    <EmptyState
      icon={SparkIcon}
      title={placeholder.title}
      hint={placeholder.hint}
    />
  );
}

import { SettingsView } from "../views/SettingsView";
import { HomeView } from "../views/HomeView";
import { InsightsView } from "../views/InsightsView";
import { DictionaryView } from "../views/DictionaryView";
import { SnippetsView } from "../views/SnippetsView";
import { StyleView } from "../views/StyleView";
import { useView } from "./ViewContext";

/**
 * Maps the active view to its screen. Every top-level destination now has a
 * real view — the switch is exhaustive over the `View` union, so adding a new
 * destination is a compile error until it is handled here.
 */
export function ViewRouter() {
  const { view } = useView();

  switch (view) {
    case "home":
      return <HomeView />;
    case "insights":
      return <InsightsView />;
    case "dictionary":
      return <DictionaryView />;
    case "snippets":
      return <SnippetsView />;
    case "style":
      return <StyleView />;
    case "settings":
      return <SettingsView />;
  }
}

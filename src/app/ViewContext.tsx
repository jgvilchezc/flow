import {
  createContext,
  useContext,
  useMemo,
  useState,
  type ReactNode,
} from "react";

/** The six top-level destinations in the management shell. */
export type View =
  | "home"
  | "insights"
  | "dictionary"
  | "snippets"
  | "style"
  | "settings";

export interface ViewContextValue {
  view: View;
  setView: (view: View) => void;
  /**
   * Monotonic counter bumped whenever a new dictation lands (single
   * `flow://history` listener in App). Views depend on it to know when to
   * refetch derived data — history, stats — without each owning a listener.
   */
  dataVersion: number;
  bumpDataVersion: () => void;
}

const ViewCtx = createContext<ViewContextValue | null>(null);

export function ViewProvider({ children }: { children: ReactNode }) {
  const [view, setView] = useState<View>("home");
  const [dataVersion, setDataVersion] = useState(0);

  const value = useMemo<ViewContextValue>(
    () => ({
      view,
      setView,
      dataVersion,
      bumpDataVersion: () => setDataVersion((v) => v + 1),
    }),
    [view, dataVersion],
  );

  return <ViewCtx.Provider value={value}>{children}</ViewCtx.Provider>;
}

export function useView(): ViewContextValue {
  const ctx = useContext(ViewCtx);
  if (!ctx) throw new Error("useView must be used within a ViewProvider");
  return ctx;
}

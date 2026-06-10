import { useEffect, useState } from "react";
import {
  getStyle,
  setActiveContext,
  setStyle,
  type StyleConfig,
  type StyleContextKey,
  type Tone,
} from "../lib/api";
import { Card } from "../components/ui/Card";
import { Badge } from "../components/ui/Badge";
import { Tabs } from "../components/ui/Tabs";
import { Spinner } from "../components/ui/Spinner";
import { cn } from "../lib/cn";

const CONTEXTS: ReadonlyArray<{ value: StyleContextKey; label: string }> = [
  { value: "personal", label: "Personal messages" },
  { value: "work", label: "Work messages" },
  { value: "email", label: "Email" },
  { value: "other", label: "Other" },
];

const PRESETS: ReadonlyArray<{ tone: Tone; title: string; desc: string }> = [
  {
    tone: "formal",
    title: "Formal",
    desc: "Caps + Punctuation",
  },
  {
    tone: "casual",
    title: "Casual",
    desc: "Caps + Less punctuation",
  },
  {
    tone: "very_casual",
    title: "Very casual",
    desc: "No caps + Less punctuation",
  },
];

const CheckIcon = (
  <svg
    width="14"
    height="14"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="2.5"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    <path d="m5 12 5 5 9-11" />
  </svg>
);

/**
 * Style management: a per-context register picker. Tabs switch between the four
 * writing contexts; within a context the three preset cards (Formal / Casual /
 * Very casual) set the tone for that context via set_style. A separate "Active
 * context" selector chooses which context's style applies to dictation right
 * now via set_active_context. State mirrors get_style.
 */
export function StyleView() {
  const [config, setConfig] = useState<StyleConfig | null>(null);
  const [tab, setTab] = useState<StyleContextKey>("personal");

  const refresh = () => getStyle().then(setConfig).catch(console.error);

  useEffect(() => {
    void refresh();
  }, []);

  if (!config) {
    return (
      <div className="flex items-center justify-center px-6 py-14">
        <Spinner label="Loading styles" />
      </div>
    );
  }

  const toneFor = (ctx: StyleContextKey): Tone =>
    config.contexts.find((c) => c.context === ctx)?.tone ?? "casual";

  const pickTone = async (tone: Tone) => {
    // Optimistic: reflect the choice immediately, then persist.
    setConfig((prev) =>
      prev
        ? {
            ...prev,
            contexts: prev.contexts.map((c) =>
              c.context === tab ? { ...c, tone } : c,
            ),
          }
        : prev,
    );
    try {
      await setStyle(tab, tone);
    } catch (e) {
      console.error(e);
      void refresh();
    }
  };

  const pickActive = async (ctx: StyleContextKey) => {
    setConfig((prev) => (prev ? { ...prev, active_context: ctx } : prev));
    try {
      await setActiveContext(ctx);
    } catch (e) {
      console.error(e);
      void refresh();
    }
  };

  const activeTone = toneFor(tab);

  return (
    <div className="px-6 py-6">
      <header className="mb-5">
        <h1 className="text-xl font-semibold tracking-tight text-text">
          Style
        </h1>
        <p className="mt-1.5 max-w-prose text-[13.5px] leading-relaxed text-muted">
          Pick a register per context. Flow applies it in whatever language you
          dictate.
        </p>
      </header>

      {/* Active context selector */}
      <Card className="mb-5 flex flex-col gap-2 p-4 sm:flex-row sm:items-center sm:justify-between">
        <div>
          <p className="text-[13.5px] font-medium text-text">Active context</p>
          <p className="text-[12.5px] text-muted">
            The style applied to your dictation right now.
          </p>
        </div>
        <Tabs
          aria-label="Active context"
          items={CONTEXTS}
          value={config.active_context}
          onChange={(c) => void pickActive(c)}
        />
      </Card>

      {/* Per-context presets */}
      <div className="mb-4">
        <Tabs
          aria-label="Context to configure"
          items={CONTEXTS}
          value={tab}
          onChange={setTab}
        />
      </div>

      <div className="grid grid-cols-1 gap-3 sm:grid-cols-3">
        {PRESETS.map((preset) => {
          const selected = preset.tone === activeTone;
          return (
            <button
              key={preset.tone}
              type="button"
              aria-pressed={selected}
              onClick={() => void pickTone(preset.tone)}
              className={cn(
                "flex flex-col gap-2 rounded-[var(--radius)] border p-4 text-left",
                "cursor-pointer outline-none transition-colors duration-150",
                "focus-visible:ring-2 focus-visible:ring-accent/40",
                selected
                  ? "border-accent bg-accent-soft/40"
                  : "border-border bg-surface hover:bg-bg",
              )}
            >
              <div className="flex items-center justify-between">
                <span className="text-[14px] font-semibold text-text">
                  {preset.title}
                </span>
                {selected && (
                  <span className="flex size-5 items-center justify-center rounded-full bg-accent text-white">
                    {CheckIcon}
                  </span>
                )}
              </div>
              <span className="text-[12.5px] leading-relaxed text-muted">
                {preset.desc}
              </span>
            </button>
          );
        })}
      </div>

      <p className="mt-4 flex items-center gap-2 text-[12.5px] text-muted">
        <Badge variant="accent">
          {CONTEXTS.find((c) => c.value === config.active_context)?.label}
        </Badge>
        is active — that context&apos;s register applies to your next dictation.
      </p>
    </div>
  );
}

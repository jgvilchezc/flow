import { useEffect, useMemo, useState } from "react";
import {
  addDictEntry,
  deleteDictEntry,
  listDictionary,
  type DictEntry,
} from "../lib/api";
import { Card } from "../components/ui/Card";
import { Button } from "../components/ui/Button";
import { Input } from "../components/ui/Input";
import { Tabs } from "../components/ui/Tabs";
import { Badge } from "../components/ui/Badge";
import { EmptyState } from "../components/ui/EmptyState";
import { Spinner } from "../components/ui/Spinner";

const BookIcon = (
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
    <path d="M5 4h11a3 3 0 0 1 3 3v13H8a3 3 0 0 1-3-3V4Zm0 0v13m3-9h7M8 12h5" />
  </svg>
);

const TrashIcon = (
  <svg
    width="15"
    height="15"
    viewBox="0 0 24 24"
    fill="none"
    stroke="currentColor"
    strokeWidth="1.6"
    strokeLinecap="round"
    strokeLinejoin="round"
    aria-hidden="true"
  >
    <path d="M4 7h16M9 7V5a1 1 0 0 1 1-1h4a1 1 0 0 1 1 1v2m-9 0 1 13h8l1-13" />
  </svg>
);

type Filter = "all" | "term" | "replacement";

const FILTERS = [
  { value: "all" as const, label: "All" },
  { value: "term" as const, label: "Terms" },
  { value: "replacement" as const, label: "Replacements" },
];

/**
 * Dictionary management: a filterable list of biasing terms and literal
 * replacements with an inline add form and per-row delete. Terms bias the STT
 * pass toward names and jargon; replacements rewrite a spoken phrase into
 * something else during post-processing.
 */
export function DictionaryView() {
  const [entries, setEntries] = useState<DictEntry[]>([]);
  const [loading, setLoading] = useState(true);
  const [filter, setFilter] = useState<Filter>("all");

  // Inline add form.
  const [adding, setAdding] = useState(false);
  const [kind, setKind] = useState<"term" | "replacement">("term");
  const [phrase, setPhrase] = useState("");
  const [replacement, setReplacement] = useState("");
  const [error, setError] = useState("");

  const refresh = () => {
    return listDictionary()
      .then(setEntries)
      .catch((e) => setError(String(e)));
  };

  useEffect(() => {
    refresh().finally(() => setLoading(false));
  }, []);

  const shown = useMemo(
    () =>
      filter === "all"
        ? entries
        : entries.filter((e) => e.kind === filter),
    [entries, filter],
  );

  const resetForm = () => {
    setAdding(false);
    setPhrase("");
    setReplacement("");
    setKind("term");
    setError("");
  };

  const submit = async () => {
    const p = phrase.trim();
    if (!p) {
      setError("Enter a word or phrase.");
      return;
    }
    if (kind === "replacement" && !replacement.trim()) {
      setError("Enter what the phrase should become.");
      return;
    }
    try {
      await addDictEntry(
        kind,
        p,
        kind === "replacement" ? replacement.trim() : undefined,
      );
      await refresh();
      resetForm();
    } catch (e) {
      setError(String(e));
    }
  };

  const remove = async (id: number | null) => {
    if (id == null) return;
    try {
      await deleteDictEntry(id);
      await refresh();
    } catch (e) {
      setError(String(e));
    }
  };

  return (
    <div className="px-6 py-6">
      <header className="mb-5 flex items-start justify-between gap-4">
        <div>
          <h1 className="text-xl font-semibold tracking-tight text-text">
            Dictionary
          </h1>
          <p className="mt-1.5 max-w-prose text-[13.5px] leading-relaxed text-muted">
            Add names and jargon so Flow spells them right, or set literal
            replacements that rewrite a spoken phrase.
          </p>
        </div>
        {!adding && (
          <Button size="sm" onClick={() => setAdding(true)}>
            Add new
          </Button>
        )}
      </header>

      <div className="mb-4">
        <Tabs
          aria-label="Filter dictionary"
          items={FILTERS}
          value={filter}
          onChange={setFilter}
        />
      </div>

      {adding && (
        <Card className="mb-4 p-4">
          <div className="flex flex-col gap-3">
            <Tabs
              aria-label="Entry kind"
              items={[
                { value: "term", label: "Term" },
                { value: "replacement", label: "Replacement" },
              ]}
              value={kind}
              onChange={(k) => setKind(k as "term" | "replacement")}
            />
            <Input
              autoFocus
              value={phrase}
              onChange={(e) => setPhrase(e.target.value)}
              placeholder={
                kind === "term"
                  ? "Word or name to spell correctly, e.g. Wispr Flow"
                  : "Phrase to replace, e.g. btw"
              }
              onKeyDown={(e) => {
                if (e.key === "Enter" && kind === "term") void submit();
              }}
            />
            {kind === "replacement" && (
              <Input
                value={replacement}
                onChange={(e) => setReplacement(e.target.value)}
                placeholder="Replace with, e.g. by the way"
                onKeyDown={(e) => {
                  if (e.key === "Enter") void submit();
                }}
              />
            )}
            {error && (
              <p role="alert" className="text-[12.5px] text-red-600">
                {error}
              </p>
            )}
            <div className="flex justify-end gap-2">
              <Button size="sm" variant="ghost" onClick={resetForm}>
                Cancel
              </Button>
              <Button size="sm" onClick={() => void submit()}>
                Add
              </Button>
            </div>
          </div>
        </Card>
      )}

      {loading ? (
        <div className="flex items-center justify-center px-6 py-14">
          <Spinner label="Loading dictionary" />
        </div>
      ) : shown.length === 0 ? (
        <EmptyState
          icon={BookIcon}
          title="Your dictionary is empty"
          hint="Add names and jargon so Flow spells them right — then they bias every transcription you dictate."
          action={
            !adding ? (
              <Button size="sm" onClick={() => setAdding(true)}>
                Add new
              </Button>
            ) : undefined
          }
        />
      ) : (
        <ul className="flex flex-col divide-y divide-border rounded-[var(--radius)] border border-border">
          {shown.map((entry) => (
            <li
              key={entry.id ?? `${entry.kind}-${entry.phrase}`}
              className="group flex items-center justify-between gap-3 px-4 py-3"
            >
              <div className="flex min-w-0 items-center gap-3">
                <Badge variant={entry.kind === "term" ? "accent" : "muted"}>
                  {entry.kind === "term" ? "Term" : "Replacement"}
                </Badge>
                <span className="truncate text-[13.5px] text-text">
                  {entry.kind === "replacement" ? (
                    <>
                      {entry.phrase}
                      <span className="text-muted"> → </span>
                      {entry.replacement}
                    </>
                  ) : (
                    entry.phrase
                  )}
                </span>
              </div>
              <button
                type="button"
                onClick={() => void remove(entry.id)}
                aria-label={`Delete ${entry.phrase}`}
                className="shrink-0 rounded-md p-1.5 text-muted opacity-0 outline-none transition hover:bg-red-50 hover:text-red-600 focus-visible:opacity-100 focus-visible:ring-2 focus-visible:ring-accent/40 group-hover:opacity-100"
              >
                {TrashIcon}
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

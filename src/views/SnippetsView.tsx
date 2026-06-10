import { useEffect, useState } from "react";
import {
  deleteSnippet,
  listSnippets,
  upsertSnippet,
  type Snippet,
} from "../lib/api";
import { Card } from "../components/ui/Card";
import { Button } from "../components/ui/Button";
import { Input } from "../components/ui/Input";
import { EmptyState } from "../components/ui/EmptyState";
import { Spinner } from "../components/ui/Spinner";

const SnippetIcon = (
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
    <path d="M8 4H6a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2v-2M9 4h6m-6 0v2a1 1 0 0 0 1 1h4a1 1 0 0 0 1-1V4m1 7-3 3 3 3m4-6 3 3-3 3" />
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

const TRUNCATE_AT = 120;

/** Reusable trigger/expansion editor used for both add and edit-in-place. */
function SnippetForm({
  initialTrigger = "",
  initialExpansion = "",
  onSave,
  onCancel,
}: {
  initialTrigger?: string;
  initialExpansion?: string;
  onSave: (trigger: string, expansion: string) => Promise<void>;
  onCancel: () => void;
}) {
  const [trigger, setTrigger] = useState(initialTrigger);
  const [expansion, setExpansion] = useState(initialExpansion);
  const [error, setError] = useState("");

  const submit = async () => {
    const t = trigger.trim();
    const x = expansion.trim();
    if (!t) {
      setError("Enter a trigger word.");
      return;
    }
    if (!x) {
      setError("Enter the text to insert.");
      return;
    }
    try {
      await onSave(t, x);
    } catch (e) {
      setError(String(e));
    }
  };

  return (
    <div className="flex flex-col gap-3">
      <Input
        autoFocus
        value={trigger}
        onChange={(e) => setTrigger(e.target.value)}
        placeholder="Trigger word, e.g. myemail"
      />
      <textarea
        value={expansion}
        onChange={(e) => setExpansion(e.target.value)}
        placeholder="Text to insert, e.g. jose@example.com"
        rows={3}
        className="w-full resize-y rounded-[var(--radius)] border border-border bg-surface px-3 py-2 text-sm text-text placeholder:text-muted outline-none transition-colors duration-150 focus-visible:border-accent focus-visible:ring-2 focus-visible:ring-accent/30"
      />
      {error && (
        <p role="alert" className="text-[12.5px] text-red-600">
          {error}
        </p>
      )}
      <div className="flex justify-end gap-2">
        <Button size="sm" variant="ghost" onClick={onCancel}>
          Cancel
        </Button>
        <Button size="sm" onClick={() => void submit()}>
          Save
        </Button>
      </div>
    </div>
  );
}

/**
 * Snippets management: short trigger words that expand into longer text as you
 * dictate. Each row shows `trigger → expansion` (truncated), with edit-in-place
 * and delete. Add and edit both go through upsert_snippet — edit passes the
 * existing id so the row is replaced rather than duplicated.
 */
export function SnippetsView() {
  const [snippets, setSnippets] = useState<Snippet[]>([]);
  const [loading, setLoading] = useState(true);
  const [adding, setAdding] = useState(false);
  const [editingId, setEditingId] = useState<number | null>(null);
  const [error, setError] = useState("");

  const refresh = () =>
    listSnippets()
      .then(setSnippets)
      .catch((e) => setError(String(e)));

  useEffect(() => {
    refresh().finally(() => setLoading(false));
  }, []);

  const save = async (trigger: string, expansion: string, id?: number) => {
    await upsertSnippet(trigger, expansion, id);
    await refresh();
    setAdding(false);
    setEditingId(null);
  };

  const remove = async (id: number | null) => {
    if (id == null) return;
    try {
      await deleteSnippet(id);
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
            Snippets
          </h1>
          <p className="mt-1.5 max-w-prose text-[13.5px] leading-relaxed text-muted">
            Say a trigger word to insert text you type often — emails,
            signatures, boilerplate.
          </p>
        </div>
        {!adding && (
          <Button size="sm" onClick={() => setAdding(true)}>
            Add new
          </Button>
        )}
      </header>

      {error && (
        <p role="alert" className="mb-4 text-[12.5px] text-red-600">
          {error}
        </p>
      )}

      {adding && (
        <Card className="mb-4 p-4">
          <SnippetForm
            onSave={(t, x) => save(t, x)}
            onCancel={() => setAdding(false)}
          />
        </Card>
      )}

      {loading ? (
        <div className="flex items-center justify-center px-6 py-14">
          <Spinner label="Loading snippets" />
        </div>
      ) : snippets.length === 0 && !adding ? (
        <EmptyState
          icon={SnippetIcon}
          title="No snippets yet"
          hint="Say a trigger word to insert text you type often. Add one and it expands the moment you speak it."
          action={
            <Button size="sm" onClick={() => setAdding(true)}>
              Add new
            </Button>
          }
        />
      ) : (
        <ul className="flex flex-col gap-3">
          {snippets.map((snippet) => {
            if (editingId === snippet.id) {
              return (
                <li key={snippet.id ?? snippet.trigger}>
                  <Card className="p-4">
                    <SnippetForm
                      initialTrigger={snippet.trigger}
                      initialExpansion={snippet.expansion}
                      onSave={(t, x) => save(t, x, snippet.id ?? undefined)}
                      onCancel={() => setEditingId(null)}
                    />
                  </Card>
                </li>
              );
            }
            const expansion =
              snippet.expansion.length > TRUNCATE_AT
                ? `${snippet.expansion.slice(0, TRUNCATE_AT)}…`
                : snippet.expansion;
            return (
              <li
                key={snippet.id ?? snippet.trigger}
                className="group flex items-center justify-between gap-3 rounded-[var(--radius)] border border-border px-4 py-3"
              >
                <button
                  type="button"
                  onClick={() => setEditingId(snippet.id)}
                  className="flex min-w-0 flex-1 items-baseline gap-2 text-left outline-none focus-visible:underline"
                >
                  <code className="shrink-0 rounded bg-accent-soft px-1.5 py-0.5 text-[12.5px] font-medium text-accent">
                    {snippet.trigger}
                  </code>
                  <span className="text-muted">→</span>
                  <span className="truncate text-[13.5px] text-text">
                    {expansion}
                  </span>
                </button>
                <button
                  type="button"
                  onClick={() => void remove(snippet.id)}
                  aria-label={`Delete ${snippet.trigger}`}
                  className="shrink-0 rounded-md p-1.5 text-muted opacity-0 outline-none transition hover:bg-red-50 hover:text-red-600 focus-visible:opacity-100 focus-visible:ring-2 focus-visible:ring-accent/40 group-hover:opacity-100"
                >
                  {TrashIcon}
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}

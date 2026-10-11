import { Cake, Pencil, Trash2, UserPlus } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { api } from "@/services/api";
import { BackendError } from "@/services/backend";
import type { PeopleView, Person } from "@/types/life";
import { lifeStyles } from "./styles";

const EMPTY = { name: "", relation: "", birthday: "", notes: "" };

function when(inDays: number): string {
  return inDays === 0
    ? "today"
    : inDays === 1
      ? "tomorrow"
      : `in ${inDays} days`;
}

/**
 * The people IGRIS knows: who they are to you, birthdays, a few notes. IGRIS
 * also adds people when you tell it about them in chat.
 */
export function PeopleList({ phone = false }: { phone?: boolean }) {
  const s = lifeStyles(phone);
  const [view, setView] = useState<PeopleView | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [form, setForm] = useState<typeof EMPTY | null>(null);
  const [confirm, setConfirm] = useState<string | null>(null);

  const load = useCallback(() => {
    api.peopleList().then(
      (v) => {
        setView(v);
        setError(null);
      },
      (err) => setError(BackendError.from(err).message),
    );
  }, []);
  useEffect(load, [load]);

  const save = () => {
    if (!form) return;
    api.personSave(form).then(
      () => {
        setForm(null);
        load();
      },
      (err) => setError(BackendError.from(err).message),
    );
  };
  const edit = (p: Person) =>
    setForm({
      name: p.name,
      relation: p.relation,
      birthday: p.birthday,
      notes: p.notes,
    });

  if (!view)
    return error ? <p className="text-sm text-danger">{error}</p> : null;
  return (
    <div className="space-y-3">
      {view.upcoming.length > 0 && (
        <div className={s.card}>
          <p className={`flex items-center gap-2 ${s.title}`}>
            <Cake className="size-4 text-accent" aria-hidden="true" /> Birthdays
            coming up
          </p>
          <ul className={`space-y-1 ${s.text}`}>
            {view.upcoming.map((u) => (
              <li key={u.person.id}>
                {u.person.name} — {when(u.inDays)}
                {u.turns ? ` (turns ${u.turns})` : ""}
              </li>
            ))}
          </ul>
        </div>
      )}

      {form ? (
        <div className={s.card}>
          <p className={s.title}>
            {view.people.some(
              (p) => p.name.toLowerCase() === form.name.trim().toLowerCase(),
            )
              ? "Edit"
              : "Add someone"}
          </p>
          <input
            className={s.input}
            aria-label="Name"
            placeholder="Name"
            maxLength={60}
            value={form.name}
            onChange={(e) => setForm({ ...form, name: e.target.value })}
          />
          <input
            className={s.input}
            aria-label="Who they are to you"
            placeholder="Who they are to you (mom, manager, friend…)"
            maxLength={60}
            value={form.relation}
            onChange={(e) => setForm({ ...form, relation: e.target.value })}
          />
          <input
            className={s.input}
            aria-label="Birthday"
            placeholder="Birthday: MM-DD or YYYY-MM-DD"
            maxLength={10}
            value={form.birthday}
            onChange={(e) => setForm({ ...form, birthday: e.target.value })}
          />
          <input
            className={s.input}
            aria-label="Notes"
            placeholder="Notes (likes, gift ideas…)"
            maxLength={500}
            value={form.notes}
            onChange={(e) => setForm({ ...form, notes: e.target.value })}
          />
          <div className="flex gap-2">
            <button
              type="button"
              className={s.chip}
              disabled={!form.name.trim()}
              onClick={save}
            >
              Save
            </button>
            <button
              type="button"
              className={s.chip}
              onClick={() => setForm(null)}
            >
              Cancel
            </button>
          </div>
        </div>
      ) : (
        <button type="button" className={s.chip} onClick={() => setForm(EMPTY)}>
          <UserPlus className="size-3.5" /> Add someone
        </button>
      )}
      {error && <p className="text-sm text-danger">{error}</p>}

      {view.people.length === 0 ? (
        <p className={s.muted}>
          Nobody yet. Tell IGRIS about people — “My sister Priya’s birthday is
          12 March, she loves books” — or add them here.
        </p>
      ) : (
        <ul className="space-y-2" aria-label="People">
          {view.people.map((p) => (
            <li
              key={p.id}
              className={
                phone ? "m-card px-4 py-3" : "rounded-2xl bg-surface px-4 py-3"
              }
            >
              <div className="flex items-start gap-3">
                <div className="min-w-0 flex-1">
                  <p className={s.title}>
                    {p.name}
                    {p.relation && (
                      <span className={`ml-2 font-normal ${s.muted}`}>
                        {p.relation}
                      </span>
                    )}
                  </p>
                  {p.birthday && (
                    <p className={s.muted}>Birthday {p.birthday}</p>
                  )}
                  {p.notes && (
                    <p className={`mt-1 break-words ${s.text}`}>{p.notes}</p>
                  )}
                </div>
                <div className="flex shrink-0 gap-1.5">
                  <button
                    type="button"
                    className={s.chip}
                    aria-label={`Edit ${p.name}`}
                    onClick={() => edit(p)}
                  >
                    <Pencil className="size-3.5" />
                  </button>
                  {confirm === p.name ? (
                    <button
                      type="button"
                      className={s.chip}
                      onClick={() =>
                        void api
                          .personForget(p.name)
                          .then(load, (err) =>
                            setError(BackendError.from(err).message),
                          )
                      }
                    >
                      <Trash2 className="size-3.5" /> Forget?
                    </button>
                  ) : (
                    <button
                      type="button"
                      className={s.chip}
                      aria-label={`Forget ${p.name}`}
                      onClick={() => setConfirm(p.name)}
                    >
                      <Trash2 className="size-3.5" />
                    </button>
                  )}
                </div>
              </div>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

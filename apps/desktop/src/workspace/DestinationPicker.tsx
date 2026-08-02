/**
 * Choosing somewhere else for one conflict.
 *
 * Opens inside the conflict dialog rather than on top of it. A second modal
 * over the first would need its own focus trap and its own escape handling, and
 * closing the wrong one is how a user loses fifteen decisions they had already
 * made. Inline keeps one trap, one Escape, and the rest of the list still on
 * screen while the choice is being made.
 *
 * The destinations offered are project-relative, because that is what the
 * import writes: the project's own folders, plus whatever the user types. A
 * path that does not exist yet is not an error — the core creates the parents —
 * so typing one is how a new folder gets made, and the picker says so rather
 * than leaving the user to guess.
 */
import { useEffect, useId, useMemo, useRef, useState } from 'react';

import type { Conflict } from './conflictResolution';
import {
  normaliseDirectory,
  previewFinalPath,
  type DestinationProblem,
  type PlanGrouping,
  type RelocationScope,
} from './relocation';

/** How the scopes read, given what the conflict actually belongs to. */
function scopeLabel(scope: RelocationScope, conflict: Conflict, grouped: boolean): string {
  switch (scope) {
    case 'one':
      return 'Only this item';
    case 'group':
      return grouped ? 'Everything in this group' : 'Only this item (it has no group)';
    case 'same-destination': {
      const cut = conflict.destination.lastIndexOf('/');
      const directory = cut < 0 ? 'the project root' : conflict.destination.slice(0, cut);
      return `Everything going to ${directory}`;
    }
    case 'all':
      return 'Every conflict that can take it';
  }
}

export default function DestinationPicker({
  conflict,
  directories,
  grouping,
  validate,
  onUse,
  onCancel,
}: {
  conflict: Conflict;
  /** Folders already in the project, offered as ready-made choices. */
  directories: string[];
  grouping: PlanGrouping;
  /** Asked on every keystroke, so the user is never told "no" only at the end. */
  validate: (destination: string, scope: RelocationScope) => DestinationProblem | null;
  onUse: (destination: string, scope: RelocationScope) => void;
  onCancel: () => void;
}) {
  const [destination, setDestination] = useState('');
  const [scope, setScope] = useState<RelocationScope>('one');
  const field = useRef<HTMLInputElement | null>(null);
  const id = useId();

  const grouped = grouping.groupOf[conflict.source] !== undefined;
  const problem = useMemo(() => validate(destination, scope), [destination, scope, validate]);
  const preview = useMemo(
    () => previewFinalPath(conflict, destination, scope, grouping),
    [conflict, destination, scope, grouping],
  );

  // The path field is where the work happens, so it takes focus on open.
  useEffect(() => {
    field.current?.focus();
  }, []);

  const trimmed = normaliseDirectory(destination);
  const isNew = trimmed !== '' && !directories.some((entry) => entry === trimmed);

  return (
    <div
      role="group"
      aria-label={`Choose another destination for ${conflict.destination}`}
      className="mt-2 border border-accent/60 bg-black/20 px-2.5 py-2"
      onKeyDown={(event) => {
        // Escape closes the picker, not the dialog: the decisions behind it are
        // still there and must not be thrown away by one keystroke.
        if (event.key === 'Escape') {
          event.preventDefault();
          event.stopPropagation();
          onCancel();
        }
      }}
    >
      <label htmlFor={`${id}-path`} className="block text-[11px] text-vs-dim">
        Destination folder
      </label>
      <div className="mt-1 flex flex-wrap items-center gap-1.5">
        <input
          id={`${id}-path`}
          ref={field}
          value={destination}
          list={`${id}-list`}
          placeholder="for example Projects/Archive"
          onChange={(event) => setDestination(event.target.value)}
          className="h-6 min-w-0 flex-1 border border-vs-border bg-vs-editor px-1.5 font-mono text-[12px] text-vs-text outline-none select-text"
        />
        <datalist id={`${id}-list`}>
          {directories.map((directory) => (
            <option key={directory} value={directory} />
          ))}
        </datalist>
        <button
          type="button"
          aria-label="Use the project root as the destination"
          onClick={() => onUse('', scope)}
          className="rounded-[2px] border border-vs-border px-2 py-0.5 text-[11px] text-vs-dim hover:text-vs-text"
        >
          Project root
        </button>
      </div>

      {directories.length > 0 && (
        <div className="mt-1.5">
          <label htmlFor={`${id}-existing`} className="text-[11px] text-vs-dim">
            Or pick a folder that already exists
          </label>
          <select
            id={`${id}-existing`}
            value={directories.includes(trimmed) ? trimmed : ''}
            onChange={(event) => setDestination(event.target.value)}
            className="ml-1.5 h-6 border border-vs-border bg-vs-editor px-1 text-[12px] text-vs-text outline-none"
          >
            <option value="">Choose…</option>
            {directories.map((directory) => (
              <option key={directory} value={directory}>
                {directory}
              </option>
            ))}
          </select>
        </div>
      )}

      <div className="mt-1.5">
        <label htmlFor={`${id}-scope`} className="text-[11px] text-vs-dim">
          Apply to
        </label>
        <select
          id={`${id}-scope`}
          value={scope}
          onChange={(event) => setScope(event.target.value as RelocationScope)}
          className="ml-1.5 h-6 border border-vs-border bg-vs-editor px-1 text-[12px] text-vs-text outline-none"
        >
          {(['one', 'group', 'same-destination', 'all'] as const).map((option) => (
            <option key={option} value={option}>
              {scopeLabel(option, conflict, grouped)}
            </option>
          ))}
        </select>
      </div>

      <p className="mt-1.5 text-[11px] text-vs-dim">
        Will land at <span className="font-mono text-accent">{preview}</span>
      </p>
      {isNew && problem === null && (
        <p className="mt-0.5 text-[11px] text-vs-dim">
          That folder does not exist yet and will be created.
        </p>
      )}
      {problem !== null && (
        <p role="alert" className="mt-0.5 text-[11px] text-amber-300">
          {problem.message}
        </p>
      )}

      <div className="mt-2 flex items-center gap-1.5">
        <button
          type="button"
          disabled={problem !== null}
          aria-label={`Use this destination for ${conflict.destination}`}
          onClick={() => onUse(destination, scope)}
          className="rounded-[2px] bg-accent px-2 py-0.5 text-[12px] font-medium text-white hover:brightness-110 disabled:cursor-not-allowed disabled:opacity-40"
        >
          Use this destination
        </button>
        <button
          type="button"
          aria-label={`Cancel choosing a destination for ${conflict.destination}`}
          onClick={onCancel}
          className="rounded-[2px] border border-vs-border px-2 py-0.5 text-[12px] text-vs-text hover:bg-white/5"
        >
          Cancel
        </button>
      </div>
    </div>
  );
}

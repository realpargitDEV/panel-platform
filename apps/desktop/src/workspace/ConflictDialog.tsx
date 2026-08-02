/**
 * The one dialog for every collision.
 *
 * Paste, drag, move and import all end up here, so a user learns the choices
 * once. It refuses to confirm while anything is unresolved — the alternative
 * is a confirm button that silently skips whatever was not answered, which is
 * how files go missing without anyone being told.
 *
 * Focus is trapped while it is open, and Explorer shortcuts stand down because
 * `isEditableTarget` treats `[role="dialog"]` as somewhere the user is working.
 */
import { useEffect, useMemo, useRef, useState } from 'react';

import Icon from './Icon';
import {
  applyToAll,
  describeConflict,
  generateName,
  normalisePath,
  setDecision,
  unresolvedCount,
  type Conflict,
  type ConflictKind,
  type Decisions,
  type ItemKind,
  type Operation,
  type PlannedItem,
  type Resolution,
} from './conflictResolution';
import {
  noGrouping,
  normaliseDirectory,
  validateRelocation,
  type PlanGrouping,
  type RelocationRequest,
  type RelocationScope,
} from './relocation';
import DestinationPicker from './DestinationPicker';

const RESOLUTION_LABEL: Record<Exclude<Resolution, 'unresolved'>, string> = {
  replace: 'Replace',
  skip: 'Skip',
  rename: 'Keep both',
  merge: 'Merge',
};

const OPERATION_LABEL: Record<Operation, string> = {
  copy: 'Copying',
  move: 'Moving',
  import: 'Importing',
};

export default function ConflictDialog({
  conflicts,
  operation,
  existing,
  items = [],
  existingKinds,
  grouping = noGrouping,
  directories = [],
  initialDecisions,
  notice = null,
  onRelocate,
  onConfirm,
  onCancel,
}: {
  conflicts: Conflict[];
  operation: Operation;
  /** Paths already at the destination, for previewing generated names. */
  existing: string[];
  /**
   * The whole batch, including the items with no conflict. Relocation needs it
   * to tell "that folder is free" from "something else is already going there".
   */
  items?: PlannedItem[];
  /** What each existing path is, so a file cannot be chosen as a destination. */
  existingKinds?: Map<string, ItemKind>;
  /** Which incoming group each item came from, when the batch was organised. */
  grouping?: PlanGrouping;
  /** Project folders the picker can offer. */
  directories?: string[];
  /** Decisions carried over from before a relocation re-analysed the batch. */
  initialDecisions?: Decisions;
  /** Set after a relocation, to say what changed. */
  notice?: string | null;
  /**
   * Apply a destination change. The caller rewrites the plan, re-runs the
   * analysis and reopens this dialog with the new conflicts, which is why there
   * is no local "relocated" state to go stale.
   */
  onRelocate?: (request: RelocationRequest, decisions: Decisions) => void;
  onConfirm: (decisions: Decisions) => void;
  onCancel: () => void;
}) {
  const [decisions, setDecisions] = useState<Decisions>(() => {
    const initial: Decisions = {};
    for (const conflict of conflicts) {
      initial[conflict.id] =
        initialDecisions?.[conflict.id] ??
        (conflict.kind === 'directory-over-directory'
          ? { resolution: 'merge' }
          : { resolution: 'unresolved' });
    }
    return initial;
  });
  const [touched, setTouched] = useState(false);
  const [confirmingCancel, setConfirmingCancel] = useState(false);
  /** The conflict whose destination picker is open, if any. */
  const [picking, setPicking] = useState<string | null>(null);

  const dialog = useRef<HTMLDivElement | null>(null);
  const restoreFocusTo = useRef<Element | null>(null);
  /** The button that opened the picker, so focus can go back to it. */
  const pickerOpenedFrom = useRef<HTMLElement | null>(null);

  const remaining = unresolvedCount(conflicts, decisions);
  const kinds = useMemo(
    () => [...new Set(conflicts.map((conflict) => conflict.kind))],
    [conflicts],
  );

  /** What each renamed item would actually be called, allocated in order. */
  const previews = useMemo(() => {
    const taken = new Set(existing.map(normalisePath));
    const names: Record<string, string> = {};
    for (const conflict of conflicts) {
      if (decisions[conflict.id]?.resolution !== 'rename') continue;
      const name = generateName(conflict.destination, taken);
      taken.add(normalisePath(name));
      names[conflict.id] = name;
    }
    return names;
  }, [conflicts, decisions, existing]);

  /**
   * What is on disk, when the caller did not say.
   *
   * `existing` is only a list of paths, so the kinds are unknown; a conflict
   * still tells us about its own. It is enough to stop a *known* file being
   * picked as a destination, which is the case worth catching here — the core
   * refuses the rest.
   */
  const inferredKinds = useMemo(() => {
    const kinds = new Map<string, ItemKind>();
    for (const conflict of conflicts) kinds.set(conflict.destination, conflict.existing);
    return kinds;
  }, [conflicts]);

  // Focus moves in when the dialog opens and back to where it was on close, so
  // the keyboard does not end up on the page behind it.
  useEffect(() => {
    restoreFocusTo.current = document.activeElement;
    dialog.current?.focus();
    return () => {
      if (restoreFocusTo.current instanceof HTMLElement) restoreFocusTo.current.focus();
    };
  }, []);

  function cancel() {
    // Escape throws away decisions. Asking first is worth it once the user has
    // made some; before that it is just a dialog in the way.
    if (touched && !confirmingCancel) {
      setConfirmingCancel(true);
      return;
    }
    onCancel();
  }

  function onKeyDown(event: React.KeyboardEvent) {
    if (event.key === 'Escape') {
      event.preventDefault();
      event.stopPropagation();
      cancel();
      return;
    }
    if (event.key !== 'Tab') return;

    // Trap: tabbing past either end wraps rather than leaving the dialog.
    const focusable = dialog.current?.querySelectorAll<HTMLElement>(
      'button:not([disabled]), [href], input, select, textarea, [tabindex]:not([tabindex="-1"])',
    );
    if (!focusable || focusable.length === 0) return;
    const first = focusable[0]!;
    const last = focusable[focusable.length - 1]!;

    if (event.shiftKey && document.activeElement === first) {
      event.preventDefault();
      last.focus();
    } else if (!event.shiftKey && document.activeElement === last) {
      event.preventDefault();
      first.focus();
    }
  }

  function choose(id: string, resolution: Resolution) {
    setTouched(true);
    setDecisions((current) => setDecision(current, id, resolution));
  }

  function chooseAll(resolution: Resolution, kind?: ConflictKind) {
    setTouched(true);
    setDecisions((current) => applyToAll(conflicts, current, resolution, kind));
  }

  /**
   * Put focus back where the picker was opened from.
   *
   * The picker is inline, so closing it removes the element the user was on. A
   * keyboard user left on `document.body` has to tab from the top of the dialog
   * to get back to the conflict they were editing.
   */
  function closePicker() {
    setPicking(null);
    const origin = pickerOpenedFrom.current;
    pickerOpenedFrom.current = null;
    // The button only exists again after this render commits.
    queueMicrotask(() => origin?.focus());
  }

  function relocate(conflict: Conflict, destination: string, scope: RelocationScope) {
    closePicker();
    // The decisions go with the request: the caller re-analyses and hands back
    // whichever of them still answer a collision that is really there.
    onRelocate?.({ conflictId: conflict.id, destination, scope }, decisions);
  }

  return (
    <div className="fixed inset-0 z-50 grid place-items-center bg-black/50 p-6">
      <div
        ref={dialog}
        role="dialog"
        aria-modal="true"
        aria-labelledby="conflict-title"
        aria-describedby="conflict-summary"
        tabIndex={-1}
        onKeyDown={onKeyDown}
        className="flex max-h-[82vh] w-[min(720px,94vw)] flex-col border border-vs-border bg-[#12182a] shadow-[0_16px_48px_rgba(0,0,0,0.6)] outline-none"
      >
        <div className="shrink-0 border-b border-vs-border px-4 py-3">
          <h2 id="conflict-title" className="text-[14px] font-semibold text-vs-text">
            {OPERATION_LABEL[operation]}: {conflicts.length} item
            {conflicts.length === 1 ? '' : 's'} already exist
          </h2>
          <p id="conflict-summary" className="mt-0.5 text-[12px] text-vs-dim">
            {remaining === 0
              ? 'Every conflict has been decided.'
              : `${remaining} still to decide. Nothing is changed until you confirm.`}
          </p>
          {notice !== null && (
            // Announced rather than only drawn: the list has just been rebuilt
            // underneath the user and the reason has to reach a screen reader.
            <p role="status" className="mt-1 text-[12px] text-amber-300">
              {notice}
            </p>
          )}
        </div>

        <div className="flex shrink-0 flex-wrap items-center gap-1.5 border-b border-vs-border px-4 py-2">
          <span className="text-[11px] text-vs-dim">Apply to all:</span>
          {(['rename', 'skip', 'replace'] as const).map((resolution) => (
            <button
              key={resolution}
              type="button"
              // Named distinctly from the per-conflict buttons: "Skip" read out
              // twice with different meanings is ambiguous by ear as well as
              // in a test.
              aria-label={`${RESOLUTION_LABEL[resolution]} for all conflicts`}
              onClick={() => chooseAll(resolution)}
              className="rounded-[2px] border border-vs-border px-2 py-0.5 text-[12px] text-vs-text hover:bg-white/5"
            >
              {RESOLUTION_LABEL[resolution]}
            </button>
          ))}
          {kinds.length > 1 &&
            kinds.map((kind) => (
              <button
                key={kind}
                type="button"
                onClick={() =>
                  chooseAll(kind === 'directory-over-directory' ? 'merge' : 'rename', kind)
                }
                title={`Apply to every ${kind.replace(/-/g, ' ')} conflict`}
                className="rounded-[2px] border border-vs-border px-2 py-0.5 text-[11px] text-vs-dim hover:bg-white/5"
              >
                {kind === 'directory-over-directory' ? 'Merge all folders' : `All ${kind}`}
              </button>
            ))}
          <span className="flex-1" />
          <button
            type="button"
            onClick={() => {
              setTouched(false);
              setDecisions(() => {
                const reset: Decisions = {};
                for (const conflict of conflicts) {
                  reset[conflict.id] =
                    conflict.kind === 'directory-over-directory'
                      ? { resolution: 'merge' }
                      : { resolution: 'unresolved' };
                }
                return reset;
              });
            }}
            className="text-[11px] text-accent hover:underline"
          >
            Reset decisions
          </button>
        </div>

        <ul className="min-h-0 flex-1 overflow-y-auto px-4 py-2">
          {conflicts.map((conflict) => {
            const chosen = decisions[conflict.id]?.resolution ?? 'unresolved';
            const unresolved = chosen === 'unresolved';

            return (
              <li
                key={conflict.id}
                className={`border-b border-vs-border/60 py-2.5 last:border-b-0 ${
                  unresolved ? '' : 'opacity-90'
                }`}
              >
                <div className="flex items-start gap-2">
                  <span className={`mt-0.5 ${unresolved ? 'text-amber-400' : 'text-vs-dim'}`}>
                    <Icon name={unresolved ? 'warning' : 'check'} size={14} />
                  </span>
                  <div className="min-w-0 flex-1">
                    <p className="truncate font-mono text-[12px] text-vs-text">
                      {conflict.destination}
                    </p>
                    <p className="mt-0.5 text-[11px] text-vs-dim">{describeConflict(conflict)}</p>
                    <p className="mt-0.5 text-[11px] text-vs-dim">
                      From <span className="font-mono">{conflict.source}</span>
                    </p>
                    {chosen === 'rename' && previews[conflict.id] && (
                      <p className="mt-0.5 text-[11px] text-accent">
                        Will be saved as <span className="font-mono">{previews[conflict.id]}</span>
                      </p>
                    )}
                    {chosen === 'skip' && (
                      <p className="mt-0.5 text-[11px] text-vs-dim">Will not be imported.</p>
                    )}
                    {chosen === 'replace' && (
                      <p className="mt-0.5 text-[11px] text-amber-400">
                        The existing item is moved aside and deleted only once the replacement is in
                        place.
                      </p>
                    )}
                  </div>

                  <div
                    role="group"
                    aria-label={`Resolution for ${conflict.destination}`}
                    className="flex shrink-0 flex-wrap gap-1"
                  >
                    {conflict.allowed.map((resolution) => (
                      <button
                        key={resolution}
                        type="button"
                        aria-pressed={chosen === resolution}
                        aria-label={`${RESOLUTION_LABEL[resolution as Exclude<Resolution, 'unresolved'>]} ${conflict.destination}`}
                        onClick={() => choose(conflict.id, resolution)}
                        className={`rounded-[2px] border px-2 py-0.5 text-[12px] ${
                          chosen === resolution
                            ? 'border-accent bg-accent/15 text-vs-text'
                            : 'border-vs-border text-vs-dim hover:text-vs-text'
                        }`}
                      >
                        {RESOLUTION_LABEL[resolution as Exclude<Resolution, 'unresolved'>]}
                      </button>
                    ))}
                    {onRelocate && (
                      <button
                        type="button"
                        aria-expanded={picking === conflict.id}
                        aria-label={`Choose another destination for ${conflict.destination}`}
                        onClick={(event) => {
                          pickerOpenedFrom.current = event.currentTarget;
                          setPicking((current) => (current === conflict.id ? null : conflict.id));
                        }}
                        className="rounded-[2px] border border-vs-border px-2 py-0.5 text-[12px] text-vs-dim hover:text-vs-text"
                      >
                        Choose another destination
                      </button>
                    )}
                  </div>
                </div>

                {picking === conflict.id && onRelocate && (
                  <DestinationPicker
                    conflict={conflict}
                    directories={directories}
                    grouping={grouping}
                    validate={(destination, scope) =>
                      validateRelocation(conflict, normaliseDirectory(destination), scope, {
                        items,
                        existing: existingKinds ?? inferredKinds,
                        grouping,
                      })
                    }
                    onUse={(destination, scope) => relocate(conflict, destination, scope)}
                    onCancel={closePicker}
                  />
                )}
              </li>
            );
          })}
        </ul>

        {confirmingCancel && (
          <p className="shrink-0 border-t border-amber-900/60 bg-amber-950/30 px-4 py-2 text-[12px] text-amber-300">
            Cancelling discards the decisions you have made. Press Cancel again to confirm.
          </p>
        )}

        <div className="flex shrink-0 items-center gap-2 border-t border-vs-border px-4 py-2.5">
          <span className="flex-1 text-[11px] text-vs-dim">
            {remaining > 0 && `${remaining} unresolved`}
          </span>
          <button
            type="button"
            onClick={cancel}
            className="rounded-[2px] border border-vs-border px-3 py-1 text-[13px] text-vs-text hover:bg-white/5"
          >
            Cancel
          </button>
          <button
            type="button"
            disabled={remaining > 0}
            title={remaining > 0 ? 'Decide every conflict first' : undefined}
            onClick={() => onConfirm(decisions)}
            className="rounded-[2px] bg-accent px-3 py-1 text-[13px] font-medium text-white hover:brightness-110 disabled:cursor-not-allowed disabled:opacity-40"
          >
            Continue
          </button>
        </div>
      </div>
    </div>
  );
}

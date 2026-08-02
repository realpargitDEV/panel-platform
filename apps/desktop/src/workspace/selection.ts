/**
 * What is selected in the explorer, and what the next click does to it.
 *
 * One model for the whole tree rather than a flag on each row. Selection rules
 * are stateful in a way that per-row state cannot express: a Shift-click needs
 * an anchor set by an earlier click, an arrow key needs to know what is visible
 * *now*, and a right-click has to decide whether it is acting on one item or on
 * the seven that were already selected.
 *
 * Paths are the identity, never indexes. Expanding a folder, refreshing a
 * listing or finishing an import all renumber the rows, and a selection stored
 * as positions silently comes to mean different files.
 */

/** One row the user can see, in the order it is drawn. */
export interface VisibleEntry {
  path: string;
  isDirectory: boolean;
}

export interface Selection {
  /** Selected paths, in no particular order. */
  selected: string[];
  /** The row the keyboard is on. Not necessarily selected. */
  focused: string | null;
  /** Where a Shift-range measures from. Survives Shift-clicks. */
  anchor: string | null;
}

export const emptySelection: Selection = { selected: [], focused: null, anchor: null };

export function isSelected(selection: Selection, path: string): boolean {
  return selection.selected.includes(path);
}

export function selectionCount(selection: Selection): number {
  return selection.selected.length;
}

/** A plain click: this row and nothing else, and ranges measure from here. */
export function selectOnly(path: string): Selection {
  return { selected: [path], focused: path, anchor: path };
}

export function clearSelection(): Selection {
  return emptySelection;
}

/**
 * Ctrl-click: add or remove one row, leaving the rest alone.
 *
 * The anchor moves to the clicked row even when the click *removed* it, which
 * is what File Explorer does — a following Shift-click measures from the last
 * place the user pointed at, not from the last row that happened to stay
 * selected.
 */
export function toggleSelection(selection: Selection, path: string): Selection {
  const selected = isSelected(selection, path)
    ? selection.selected.filter((entry) => entry !== path)
    : [...selection.selected, path];
  return { selected, focused: path, anchor: path };
}

/**
 * Shift-click: everything between the anchor and here.
 *
 * The anchor is deliberately *not* moved, so a second Shift-click re-measures
 * from the same origin and can shrink the range as well as grow it. With no
 * anchor yet — the first thing the user does is Shift-click — it behaves as a
 * plain click, which is the only sensible reading.
 */
export function extendSelection(
  selection: Selection,
  path: string,
  visible: VisibleEntry[],
): Selection {
  if (selection.anchor === null) return selectOnly(path);

  const order = visible.map((entry) => entry.path);
  const from = order.indexOf(selection.anchor);
  const to = order.indexOf(path);
  // The anchor scrolled out of view, or its folder was collapsed. Falling back
  // to a plain click beats selecting a range measured from nowhere.
  if (from < 0 || to < 0) return selectOnly(path);

  const [start, end] = from <= to ? [from, to] : [to, from];
  return {
    selected: order.slice(start, end + 1),
    focused: path,
    anchor: selection.anchor,
  };
}

/**
 * Ctrl+Shift-click: add the range to what is already selected.
 *
 * File Explorer's behaviour for building a selection out of several runs.
 */
export function addRangeToSelection(
  selection: Selection,
  path: string,
  visible: VisibleEntry[],
): Selection {
  const range = extendSelection(selection, path, visible);
  return {
    selected: [...new Set([...selection.selected, ...range.selected])],
    focused: path,
    anchor: selection.anchor ?? path,
  };
}

export function selectAll(visible: VisibleEntry[]): Selection {
  if (visible.length === 0) return emptySelection;
  const selected = visible.map((entry) => entry.path);
  return {
    selected,
    focused: selected[selected.length - 1] ?? null,
    anchor: selected[0] ?? null,
  };
}

export function selectPaths(paths: string[]): Selection {
  const [first] = paths;
  if (first === undefined) return emptySelection;
  return { selected: [...paths], focused: paths[paths.length - 1] ?? null, anchor: first };
}

/** How a keyboard move treats the existing selection. */
export type FocusMode =
  /** Arrow: move and select only the new row. */
  | 'replace'
  /** Shift+Arrow: move and extend from the anchor. */
  | 'extend'
  /** Ctrl+Arrow: move the focus, touch nothing else. */
  | 'keep';

/**
 * Arrow-key movement.
 *
 * Clamped rather than wrapped: reaching the bottom and pressing Down again in a
 * file tree should stay at the bottom, not jump to the top.
 */
export function moveFocus(
  selection: Selection,
  visible: VisibleEntry[],
  delta: number,
  mode: FocusMode,
): Selection {
  if (visible.length === 0) return selection;

  const order = visible.map((entry) => entry.path);
  const current = selection.focused === null ? -1 : order.indexOf(selection.focused);
  // Nothing focused yet: Down starts at the top, Up starts at the bottom.
  const next =
    current < 0
      ? delta > 0
        ? 0
        : order.length - 1
      : Math.max(0, Math.min(order.length - 1, current + delta));

  const path = order[next];
  if (path === undefined) return selection;
  return applyFocus(selection, path, visible, mode);
}

export function focusEdge(
  selection: Selection,
  visible: VisibleEntry[],
  edge: 'first' | 'last',
  mode: FocusMode,
): Selection {
  const path = edge === 'first' ? visible[0]?.path : visible[visible.length - 1]?.path;
  if (path === undefined) return selection;
  return applyFocus(selection, path, visible, mode);
}

function applyFocus(
  selection: Selection,
  path: string,
  visible: VisibleEntry[],
  mode: FocusMode,
): Selection {
  switch (mode) {
    case 'extend':
      return extendSelection(
        { ...selection, anchor: selection.anchor ?? selection.focused },
        path,
        visible,
      );
    case 'keep':
      return { ...selection, focused: path };
    default:
      return selectOnly(path);
  }
}

/** The modifier keys a pointer press carried. */
export interface Modifiers {
  ctrl: boolean;
  shift: boolean;
}

/**
 * What a press on a row does to the selection.
 *
 * The whole rule in one place, so the tree, the import organiser and the tests
 * all agree. Applied on the press rather than the click, because a drag that
 * begins here must already be carrying the right set.
 *
 * A plain press *inside* an existing multi-selection keeps it — otherwise
 * dragging a group of seven would silently reduce it to the one row under the
 * pointer. A plain press outside starts a new selection.
 */
export function selectFromPointer(
  selection: Selection,
  path: string,
  visible: VisibleEntry[],
  modifiers: Modifiers,
): Selection {
  if (modifiers.shift && modifiers.ctrl) return addRangeToSelection(selection, path, visible);
  if (modifiers.ctrl) return toggleSelection(selection, path);
  if (modifiers.shift) return extendSelection(selection, path, visible);
  if (!isSelected(selection, path)) return selectOnly(path);
  return { ...selection, focused: path };
}

/**
 * What a right-click acts on.
 *
 * Right-clicking inside a selection keeps it — otherwise every context menu on
 * a multi-selection would silently reduce it to one file and the "Delete 7
 * items" the user was reaching for would delete one. Right-clicking outside a
 * selection replaces it, because acting on rows the user cannot see they
 * selected is worse.
 */
export function selectionForContextMenu(selection: Selection, path: string): Selection {
  return isSelected(selection, path) ? { ...selection, focused: path } : selectOnly(path);
}

/**
 * What a drag carries.
 *
 * The same rule as the context menu: dragging one of several selected rows
 * moves all of them; dragging an unselected row moves only it.
 */
export function dragPaths(selection: Selection, path: string): string[] {
  return isSelected(selection, path) ? [...selection.selected] : [path];
}

/**
 * Drop paths that are no longer there.
 *
 * Called after a refresh, a delete or an import. A selection that keeps naming
 * deleted files reports "6 selected" over four rows, and the next Delete asks
 * about files that are already gone.
 */
export function pruneSelection(selection: Selection, existing: string[]): Selection {
  const alive = new Set(existing);
  const selected = selection.selected.filter((path) => alive.has(path));
  return {
    selected,
    focused: selection.focused !== null && alive.has(selection.focused) ? selection.focused : null,
    anchor: selection.anchor !== null && alive.has(selection.anchor) ? selection.anchor : null,
  };
}

/**
 * Follow a rename or a move.
 *
 * Matches the path itself and anything beneath it, so renaming a folder keeps
 * the selection on the files inside it rather than dropping them.
 */
export function renameInSelection(selection: Selection, from: string, to: string): Selection {
  const moved = (path: string) =>
    path === from ? to : path.startsWith(`${from}/`) ? `${to}${path.slice(from.length)}` : path;

  return {
    selected: selection.selected.map(moved),
    focused: selection.focused === null ? null : moved(selection.focused),
    anchor: selection.anchor === null ? null : moved(selection.anchor),
  };
}

/**
 * Whether a keystroke belongs to the explorer or to whatever the user is
 * typing in.
 *
 * Delete inside a rename box deletes a character; the same key on the tree
 * deletes files. Getting this backwards destroys work, so the check is
 * deliberate and shared.
 */
export function isEditableTarget(target: unknown): boolean {
  if (target === null || typeof target !== 'object') return false;
  const candidate = target as { closest?: (selector: string) => unknown };
  if (typeof candidate.closest !== 'function') return false;
  return (
    candidate.closest(
      'input, textarea, select, [contenteditable="true"], .monaco-editor, [role="dialog"]',
    ) !== null
  );
}

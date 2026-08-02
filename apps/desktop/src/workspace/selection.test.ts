import { describe, expect, it } from 'vitest';

import {
  addRangeToSelection,
  clearSelection,
  dragPaths,
  emptySelection,
  extendSelection,
  focusEdge,
  isEditableTarget,
  isSelected,
  moveFocus,
  pruneSelection,
  renameInSelection,
  selectAll,
  selectOnly,
  selectPaths,
  selectionForContextMenu,
  toggleSelection,
  type Selection,
  type VisibleEntry,
} from './selection';

/** The tree as drawn: src/, its two children, then two root files. */
const visible: VisibleEntry[] = [
  { path: 'src', isDirectory: true },
  { path: 'src/a.ts', isDirectory: false },
  { path: 'src/b.ts', isDirectory: false },
  { path: 'README.md', isDirectory: false },
  { path: 'package.json', isDirectory: false },
];

function withSelected(paths: string[], anchor: string | null = paths[0] ?? null): Selection {
  return { selected: paths, focused: paths[paths.length - 1] ?? null, anchor };
}

describe('clicking', () => {
  it('selects only what was clicked', () => {
    const selection = selectOnly('src/a.ts');
    expect(selection.selected).toEqual(['src/a.ts']);
    expect(selection.anchor).toBe('src/a.ts');
    expect(selection.focused).toBe('src/a.ts');
  });

  it('clears everything', () => {
    expect(clearSelection()).toEqual(emptySelection);
  });
});

describe('ctrl-clicking', () => {
  it('adds an item without disturbing the others', () => {
    const selection = toggleSelection(withSelected(['src/a.ts']), 'README.md');
    expect(selection.selected).toEqual(['src/a.ts', 'README.md']);
  });

  it('removes an item that was already selected', () => {
    const selection = toggleSelection(withSelected(['src/a.ts', 'README.md']), 'src/a.ts');
    expect(selection.selected).toEqual(['README.md']);
  });

  it('moves the anchor even when it removed the item', () => {
    // File Explorer measures the next Shift-range from where the user last
    // pointed, not from the last row that happened to stay selected.
    const selection = toggleSelection(withSelected(['src/a.ts', 'README.md']), 'README.md');
    expect(selection.anchor).toBe('README.md');
    expect(selection.selected).toEqual(['src/a.ts']);
  });

  it('can empty the selection entirely', () => {
    expect(toggleSelection(withSelected(['src/a.ts']), 'src/a.ts').selected).toEqual([]);
  });
});

describe('shift-clicking', () => {
  it('selects the run between the anchor and the click', () => {
    const selection = extendSelection(withSelected(['src']), 'src/b.ts', visible);
    expect(selection.selected).toEqual(['src', 'src/a.ts', 'src/b.ts']);
  });

  it('works upwards as well as downwards', () => {
    const selection = extendSelection(withSelected(['README.md']), 'src/a.ts', visible);
    expect(selection.selected).toEqual(['src/a.ts', 'src/b.ts', 'README.md']);
  });

  it('keeps the anchor, so a second shift-click can shrink the range', () => {
    const first = extendSelection(withSelected(['src']), 'package.json', visible);
    expect(first.selected).toHaveLength(5);
    const second = extendSelection(first, 'src/a.ts', visible);
    expect(second.selected).toEqual(['src', 'src/a.ts']);
    expect(second.anchor).toBe('src');
  });

  it('behaves as a plain click when there is no anchor yet', () => {
    expect(extendSelection(emptySelection, 'README.md', visible).selected).toEqual(['README.md']);
  });

  it('falls back to a plain click when the anchor is no longer visible', () => {
    // Its folder was collapsed. A range measured from nowhere is worse.
    const selection = extendSelection(withSelected(['gone/file.ts']), 'README.md', visible);
    expect(selection.selected).toEqual(['README.md']);
  });

  it('adds a second run when ctrl and shift are held together', () => {
    const first = withSelected(['package.json'], 'package.json');
    const selection = addRangeToSelection({ ...first, anchor: 'src' }, 'src/a.ts', visible);
    expect(selection.selected).toEqual(['package.json', 'src', 'src/a.ts']);
  });
});

describe('select all', () => {
  it('takes every visible row', () => {
    expect(selectAll(visible).selected).toEqual(visible.map((entry) => entry.path));
  });

  it('does nothing to an empty tree', () => {
    expect(selectAll([])).toEqual(emptySelection);
  });
});

describe('keyboard movement', () => {
  it('moves down and selects only the new row', () => {
    const selection = moveFocus(withSelected(['src']), visible, 1, 'replace');
    expect(selection.focused).toBe('src/a.ts');
    expect(selection.selected).toEqual(['src/a.ts']);
  });

  it('extends from the anchor with shift', () => {
    const selection = moveFocus(withSelected(['src']), visible, 1, 'extend');
    expect(selection.selected).toEqual(['src', 'src/a.ts']);
  });

  it('moves focus alone with ctrl, leaving the selection intact', () => {
    const before = withSelected(['src/a.ts', 'README.md']);
    const selection = moveFocus({ ...before, focused: 'src/a.ts' }, visible, 1, 'keep');
    expect(selection.focused).toBe('src/b.ts');
    expect(selection.selected).toEqual(['src/a.ts', 'README.md']);
  });

  it('stops at the ends rather than wrapping around', () => {
    const bottom = withSelected(['package.json']);
    expect(moveFocus(bottom, visible, 1, 'replace').focused).toBe('package.json');
    const top = withSelected(['src']);
    expect(moveFocus(top, visible, -1, 'replace').focused).toBe('src');
  });

  it('starts at the top when nothing is focused and the user presses down', () => {
    expect(moveFocus(emptySelection, visible, 1, 'replace').focused).toBe('src');
    expect(moveFocus(emptySelection, visible, -1, 'replace').focused).toBe('package.json');
  });

  it('jumps to the first and last rows', () => {
    expect(focusEdge(emptySelection, visible, 'first', 'replace').focused).toBe('src');
    expect(focusEdge(emptySelection, visible, 'last', 'replace').focused).toBe('package.json');
  });

  it('does nothing in an empty tree', () => {
    expect(moveFocus(emptySelection, [], 1, 'replace')).toEqual(emptySelection);
    expect(focusEdge(emptySelection, [], 'first', 'replace')).toEqual(emptySelection);
  });
});

describe('right-clicking', () => {
  it('keeps a multi-selection when the click lands inside it', () => {
    // Otherwise "Delete" on a menu opened over seven selected files deletes one.
    const before = withSelected(['src/a.ts', 'README.md']);
    const selection = selectionForContextMenu(before, 'README.md');
    expect(selection.selected).toEqual(['src/a.ts', 'README.md']);
  });

  it('replaces the selection when the click lands outside it', () => {
    const before = withSelected(['src/a.ts', 'README.md']);
    expect(selectionForContextMenu(before, 'package.json').selected).toEqual(['package.json']);
  });
});

describe('dragging', () => {
  it('carries the whole selection when one of its items is dragged', () => {
    expect(dragPaths(withSelected(['src/a.ts', 'README.md']), 'src/a.ts')).toEqual([
      'src/a.ts',
      'README.md',
    ]);
  });

  it('carries only the dragged item when it was not selected', () => {
    expect(dragPaths(withSelected(['src/a.ts']), 'package.json')).toEqual(['package.json']);
  });
});

describe('surviving a refresh', () => {
  it('drops paths that no longer exist', () => {
    const before = withSelected(['src/a.ts', 'gone.ts']);
    const after = pruneSelection(before, ['src/a.ts', 'README.md']);
    expect(after.selected).toEqual(['src/a.ts']);
  });

  it('clears a focus and anchor that were deleted', () => {
    const before: Selection = { selected: ['gone.ts'], focused: 'gone.ts', anchor: 'gone.ts' };
    const after = pruneSelection(before, ['README.md']);
    expect(after).toEqual({ selected: [], focused: null, anchor: null });
  });

  it('keeps a selection that is entirely still there', () => {
    const before = withSelected(['src/a.ts', 'README.md']);
    expect(
      pruneSelection(
        before,
        visible.map((entry) => entry.path),
      ).selected,
    ).toEqual(before.selected);
  });
});

describe('following a rename', () => {
  it('renames the item itself', () => {
    expect(renameInSelection(withSelected(['a.ts']), 'a.ts', 'b.ts').selected).toEqual(['b.ts']);
  });

  it('renames everything underneath a renamed folder', () => {
    const before = withSelected(['src/a.ts', 'src/deep/b.ts']);
    const after = renameInSelection(before, 'src', 'source');
    expect(after.selected).toEqual(['source/a.ts', 'source/deep/b.ts']);
  });

  it('leaves unrelated paths alone, including a name that merely starts the same', () => {
    const before = withSelected(['srcfile.ts']);
    expect(renameInSelection(before, 'src', 'source').selected).toEqual(['srcfile.ts']);
  });

  it('moves the focus and anchor with it', () => {
    const before: Selection = { selected: ['src/a.ts'], focused: 'src/a.ts', anchor: 'src/a.ts' };
    const after = renameInSelection(before, 'src/a.ts', 'src/c.ts');
    expect(after.focused).toBe('src/c.ts');
    expect(after.anchor).toBe('src/c.ts');
  });
});

describe('selecting a known set of paths', () => {
  it('selects them and anchors on the first', () => {
    const selection = selectPaths(['a', 'b', 'c']);
    expect(selection.selected).toEqual(['a', 'b', 'c']);
    expect(selection.anchor).toBe('a');
    expect(selection.focused).toBe('c');
  });

  it('is empty for an empty list', () => {
    expect(selectPaths([])).toEqual(emptySelection);
  });
});

describe('deciding whether a keystroke is the explorer’s', () => {
  function target(matches: string[]) {
    return {
      closest: (selector: string) => (matches.includes(selector) ? {} : null),
    };
  }

  const EDITABLE =
    'input, textarea, select, [contenteditable="true"], .monaco-editor, [role="dialog"]';

  it('stands down inside a text box, the editor or a dialog', () => {
    expect(isEditableTarget(target([EDITABLE]))).toBe(true);
  });

  it('takes the keystroke on an ordinary row', () => {
    expect(isEditableTarget(target([]))).toBe(false);
  });

  it('never throws on a target that cannot be asked', () => {
    // A keydown dispatched at the window has no `closest`.
    expect(isEditableTarget(globalThis)).toBe(false);
    expect(isEditableTarget(null)).toBe(false);
    expect(isEditableTarget(undefined)).toBe(false);
  });
});

describe('membership', () => {
  it('reports what is selected', () => {
    expect(isSelected(withSelected(['a', 'b']), 'b')).toBe(true);
    expect(isSelected(withSelected(['a', 'b']), 'c')).toBe(false);
  });
});

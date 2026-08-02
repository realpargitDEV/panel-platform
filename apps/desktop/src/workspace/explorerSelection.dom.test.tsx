/**
 * The explorer's selection, as rendered.
 *
 * These drive the real `FileTree` through real events and assert on what the
 * user can see — `aria-selected` on the rows — rather than on the shape of the
 * state behind it. The harness holds the selection and uses the same
 * `selectFromPointer` the workspace does, so a change that breaks the wiring
 * breaks these too.
 */
import { fireEvent, render, screen, within } from '@testing-library/react';
import { useState } from 'react';
import { describe, expect, it, vi } from 'vitest';

import type { FileEntry } from '../api';
import FileTree, { type TreeState } from './FileTree';
import {
  emptySelection,
  selectFromPointer,
  selectionForContextMenu,
  dragPaths,
  selectAll,
  clearSelection,
  pruneSelection,
  type Selection,
  type VisibleEntry,
} from './selection';

function entry(path: string, kind: FileEntry['kind'] = 'file'): FileEntry {
  return {
    name: path.slice(path.lastIndexOf('/') + 1),
    path,
    kind,
    sizeBytes: 0,
    modifiedUnixMs: null,
    isSymlink: false,
  };
}

const listings: Record<string, FileEntry[]> = {
  '': [entry('src', 'directory'), entry('README.md'), entry('package.json')],
  src: [entry('src/a.ts'), entry('src/b.ts')],
};

/** The rows as drawn with `src` expanded. */
const visible: VisibleEntry[] = [
  { path: 'src', isDirectory: true },
  { path: 'src/a.ts', isDirectory: false },
  { path: 'src/b.ts', isDirectory: false },
  { path: 'README.md', isDirectory: false },
  { path: 'package.json', isDirectory: false },
];

function Harness({
  onOpen = () => {},
  onDelete = () => {},
  initialExpanded = ['src'],
  onSelectionChange,
}: {
  onOpen?: (path: string) => void;
  onDelete?: (paths: string[]) => void;
  initialExpanded?: string[];
  onSelectionChange?: (selection: Selection) => void;
} = {}) {
  const [selection, setSelectionState] = useState<Selection>(emptySelection);
  const [expanded, setExpanded] = useState<string[]>(initialExpanded);

  function setSelection(next: Selection) {
    setSelectionState(next);
    onSelectionChange?.(next);
  }

  const rows: VisibleEntry[] = [];
  const walk = (directory: string) => {
    for (const item of listings[directory] ?? []) {
      rows.push({ path: item.path, isDirectory: item.kind === 'directory' });
      if (item.kind === 'directory' && expanded.includes(item.path)) walk(item.path);
    }
  };
  walk('');

  const state: TreeState = {
    listings,
    expanded,
    selection,
    targetDirectory: '',
    editing: null,
  };

  return (
    <div
      onKeyDown={(event) => {
        if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 'a') {
          event.preventDefault();
          setSelection(selectAll(rows));
        } else if (event.key === 'Escape') {
          setSelection(clearSelection());
        } else if (event.key === 'Delete') {
          onDelete(selection.selected);
        }
      }}
    >
      <FileTree
        directory=""
        depth={0}
        state={state}
        callbacks={{
          onOpen: (item) => onOpen(item.path),
          onToggle: (path) =>
            setExpanded((current) =>
              current.includes(path)
                ? current.filter((entry) => entry !== path)
                : [...current, path],
            ),
          onContextMenu: (item, event) => {
            event.preventDefault();
            setSelection(selectionForContextMenu(selection, item.path));
          },
          onRowPointerDown: (item, event) =>
            setSelection(
              selectFromPointer(selection, item.path, rows, {
                ctrl: event.ctrlKey || event.metaKey,
                shift: event.shiftKey,
              }),
            ),
          onMove: () => {},
          dragPathsFor: (path) => dragPaths(selection, path),
          onSelectDirectory: () => {},
        }}
        onSubmitEdit={() => {}}
        onCancelEdit={() => {}}
      />
    </div>
  );
}

function row(path: string) {
  return document.querySelector(`[data-path="${CSS.escape(path)}"]`) as HTMLElement;
}

function selectedPaths(): string[] {
  return [...document.querySelectorAll('[role="treeitem"][aria-selected="true"]')].map(
    (element) => (element as HTMLElement).dataset.path ?? '',
  );
}

describe('clicking rows', () => {
  it('selects only the row that was clicked', () => {
    render(<Harness />);
    fireEvent.mouseDown(row('README.md'));
    expect(selectedPaths()).toEqual(['README.md']);
  });

  it('moves the selection to the next row clicked', () => {
    render(<Harness />);
    fireEvent.mouseDown(row('README.md'));
    fireEvent.mouseDown(row('package.json'));
    expect(selectedPaths()).toEqual(['package.json']);
  });

  it('adds and removes with ctrl-click', () => {
    render(<Harness />);
    fireEvent.mouseDown(row('README.md'));
    fireEvent.mouseDown(row('package.json'), { ctrlKey: true });
    expect(selectedPaths()).toEqual(['README.md', 'package.json']);

    fireEvent.mouseDown(row('README.md'), { ctrlKey: true });
    expect(selectedPaths()).toEqual(['package.json']);
  });

  it('selects a visible range with shift-click', () => {
    render(<Harness />);
    fireEvent.mouseDown(row('src'));
    fireEvent.mouseDown(row('README.md'), { shiftKey: true });
    expect(selectedPaths()).toEqual(['src', 'src/a.ts', 'src/b.ts', 'README.md']);
  });

  it('keeps the anchor so a second shift-click shrinks the range', () => {
    render(<Harness />);
    fireEvent.mouseDown(row('src'));
    fireEvent.mouseDown(row('package.json'), { shiftKey: true });
    expect(selectedPaths()).toHaveLength(5);

    fireEvent.mouseDown(row('src/a.ts'), { shiftKey: true });
    expect(selectedPaths()).toEqual(['src', 'src/a.ts']);
  });

  it('opens a file on a plain click', () => {
    const onOpen = vi.fn();
    render(<Harness onOpen={onOpen} />);
    fireEvent.mouseDown(row('README.md'));
    fireEvent.click(row('README.md'));
    expect(onOpen).toHaveBeenCalledWith('README.md');
  });

  it('does not open a file when the click was only changing the selection', () => {
    const onOpen = vi.fn();
    render(<Harness onOpen={onOpen} />);
    fireEvent.mouseDown(row('README.md'), { ctrlKey: true });
    fireEvent.click(row('README.md'), { ctrlKey: true });
    expect(onOpen).not.toHaveBeenCalled();
  });
});

describe('right-clicking', () => {
  it('keeps a multi-selection when the click lands inside it', () => {
    render(<Harness />);
    fireEvent.mouseDown(row('README.md'));
    fireEvent.mouseDown(row('package.json'), { ctrlKey: true });
    fireEvent.contextMenu(row('README.md'));
    expect(selectedPaths()).toEqual(['README.md', 'package.json']);
  });

  it('selects an unselected row before opening the menu', () => {
    render(<Harness />);
    fireEvent.mouseDown(row('README.md'));
    fireEvent.contextMenu(row('package.json'));
    expect(selectedPaths()).toEqual(['package.json']);
  });
});

describe('keyboard', () => {
  it('selects every visible row with ctrl+A', () => {
    render(<Harness />);
    fireEvent.keyDown(row('README.md'), { key: 'a', ctrlKey: true });
    expect(selectedPaths()).toEqual(['src', 'src/a.ts', 'src/b.ts', 'README.md', 'package.json']);
  });

  it('does not select rows hidden inside a collapsed folder', () => {
    render(<Harness initialExpanded={[]} />);
    fireEvent.keyDown(row('README.md'), { key: 'a', ctrlKey: true });
    expect(selectedPaths()).toEqual(['src', 'README.md', 'package.json']);
  });

  it('clears the selection with escape', () => {
    render(<Harness />);
    fireEvent.mouseDown(row('README.md'));
    fireEvent.keyDown(row('README.md'), { key: 'Escape' });
    expect(selectedPaths()).toEqual([]);
  });

  it('asks to delete the whole selection at once', () => {
    const onDelete = vi.fn();
    render(<Harness onDelete={onDelete} />);
    fireEvent.mouseDown(row('README.md'));
    fireEvent.mouseDown(row('package.json'), { ctrlKey: true });
    fireEvent.keyDown(row('package.json'), { key: 'Delete' });

    // One call carrying both paths, not one call per file.
    expect(onDelete).toHaveBeenCalledTimes(1);
    expect(onDelete).toHaveBeenCalledWith(['README.md', 'package.json']);
  });
});

describe('collapsing a folder', () => {
  it('does not corrupt a path-based selection', () => {
    render(<Harness />);
    fireEvent.mouseDown(row('src/a.ts'));
    expect(selectedPaths()).toEqual(['src/a.ts']);

    // Collapsing hides the row; the selection still names the same file.
    fireEvent.click(row('src'));
    expect(row('src/a.ts')).toBeNull();

    fireEvent.click(row('src'));
    expect(selectedPaths()).toEqual(['src/a.ts']);
  });
});

describe('dragging', () => {
  it('carries the whole selection when a selected row is dragged', () => {
    render(<Harness />);
    fireEvent.mouseDown(row('README.md'));
    fireEvent.mouseDown(row('package.json'), { ctrlKey: true });

    const data: Record<string, string> = {};
    fireEvent.dragStart(row('README.md'), {
      dataTransfer: {
        setData: (type: string, value: string) => {
          data[type] = value;
        },
        setDragImage: () => {},
        types: [],
      },
    });

    expect(JSON.parse(data['application/x-project-path'] ?? '[]')).toEqual([
      'README.md',
      'package.json',
    ]);
  });

  it('carries only the row dragged when it was not selected', () => {
    render(<Harness />);
    fireEvent.mouseDown(row('README.md'));

    const data: Record<string, string> = {};
    fireEvent.dragStart(row('package.json'), {
      dataTransfer: {
        setData: (type: string, value: string) => {
          data[type] = value;
        },
        setDragImage: () => {},
        types: [],
      },
    });

    expect(JSON.parse(data['application/x-project-path'] ?? '[]')).toEqual(['package.json']);
  });

  it('reports a move to the tree with every dragged path', () => {
    const onMove = vi.fn();
    function MoveHarness() {
      const state: TreeState = {
        listings,
        expanded: ['src'],
        selection: emptySelection,
        targetDirectory: '',
        editing: null,
      };
      return (
        <FileTree
          directory=""
          depth={0}
          state={state}
          callbacks={{
            onOpen: () => {},
            onToggle: () => {},
            onContextMenu: () => {},
            onRowPointerDown: () => {},
            onMove,
            dragPathsFor: () => [],
            onSelectDirectory: () => {},
          }}
          onSubmitEdit={() => {}}
          onCancelEdit={() => {}}
        />
      );
    }
    render(<MoveHarness />);

    const payload = JSON.stringify(['README.md', 'package.json']);
    fireEvent.drop(row('src'), {
      dataTransfer: {
        types: ['application/x-project-path'],
        getData: () => payload,
      },
    });

    expect(onMove).toHaveBeenCalledWith(['README.md', 'package.json'], 'src');
  });
});

describe('pruning', () => {
  it('drops paths that are no longer rendered', () => {
    // The rule the tree relies on after a delete or a refresh.
    const before: Selection = {
      selected: ['README.md', 'gone.ts'],
      focused: 'gone.ts',
      anchor: 'README.md',
    };
    const after = pruneSelection(
      before,
      visible.map((item) => item.path),
    );
    expect(after.selected).toEqual(['README.md']);
    expect(after.focused).toBeNull();
  });
});

describe('the tree as rendered', () => {
  it('marks selected rows for assistive technology, not only visually', () => {
    render(<Harness />);
    fireEvent.mouseDown(row('README.md'));
    const tree = screen.getByRole('tree');
    expect(within(tree).getAllByRole('treeitem', { selected: true })).toHaveLength(1);
  });
});

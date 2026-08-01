import { describe, expect, it } from 'vitest';
import {
  activeBuffer,
  childPath,
  closeAll,
  closeFile,
  closeOthers,
  closeToRight,
  dirtyPaths,
  edit,
  emptyEditor,
  forgetDeleted,
  isDirty,
  markSaved,
  openFile,
  parentOf,
  renamePath,
  tabLabel,
  toggleExpanded,
  type EditorState,
} from './tabs';

function file(path: string, text = 'original') {
  return { path, text, language: 'typescript', readOnly: false };
}

/** Three files open, the last one active. */
function threeOpen(): EditorState {
  let state = emptyEditor;
  state = openFile(state, file('a.ts'));
  state = openFile(state, file('b.ts'));
  state = openFile(state, file('c.ts'));
  return state;
}

describe('opening files', () => {
  it('opens a file and makes it active', () => {
    const state = openFile(emptyEditor, file('src/index.ts', 'hello'));
    expect(state.active).toBe('src/index.ts');
    expect(activeBuffer(state)?.current).toBe('hello');
    expect(isDirty(activeBuffer(state)!)).toBe(false);
  });

  it('focuses an already-open file rather than opening it twice', () => {
    let state = threeOpen();
    state = openFile(state, file('a.ts'));
    expect(state.buffers).toHaveLength(3);
    expect(state.active).toBe('a.ts');
  });

  it('keeps unsaved edits when a file is reopened', () => {
    // Clicking the same file in the tree must not throw away typing.
    let state = openFile(emptyEditor, file('a.ts', 'original'));
    state = edit(state, 'a.ts', 'edited');
    state = openFile(state, file('a.ts', 'original'));
    expect(activeBuffer(state)?.current).toBe('edited');
    expect(dirtyPaths(state)).toEqual(['a.ts']);
  });
});

describe('dirty tracking', () => {
  it('marks a buffer dirty once it differs from what was saved', () => {
    let state = openFile(emptyEditor, file('a.ts', 'x'));
    expect(dirtyPaths(state)).toEqual([]);
    state = edit(state, 'a.ts', 'x!');
    expect(dirtyPaths(state)).toEqual(['a.ts']);
  });

  it('treats a buffer typed back to its original text as clean', () => {
    // Otherwise the close guard would nag about a file that matches the disk.
    let state = openFile(emptyEditor, file('a.ts', 'x'));
    state = edit(state, 'a.ts', 'x!');
    state = edit(state, 'a.ts', 'x');
    expect(dirtyPaths(state)).toEqual([]);
  });

  it('clears the marker for the file that was saved and no other', () => {
    let state = threeOpen();
    state = edit(state, 'a.ts', 'a edited');
    state = edit(state, 'b.ts', 'b edited');
    state = markSaved(state, 'a.ts', 'a edited');
    expect(dirtyPaths(state)).toEqual(['b.ts']);
  });

  it('leaves a buffer dirty when typing continued during the save', () => {
    // The save wrote the older text; the newer characters are not on disk, and
    // saying otherwise would lose them silently.
    let state = openFile(emptyEditor, file('a.ts', 'v1'));
    state = edit(state, 'a.ts', 'v2');
    const written = activeBuffer(state)!.current;
    state = edit(state, 'a.ts', 'v3');
    state = markSaved(state, 'a.ts', written);
    expect(dirtyPaths(state)).toEqual(['a.ts']);
    expect(activeBuffer(state)?.current).toBe('v3');
  });
});

describe('closing tabs', () => {
  it('moves focus to the tab on the left', () => {
    const state = closeFile(threeOpen(), 'c.ts');
    expect(state.active).toBe('b.ts');
    expect(state.buffers.map((buffer) => buffer.path)).toEqual(['a.ts', 'b.ts']);
  });

  it('moves focus right when the first tab is closed', () => {
    let state = threeOpen();
    state = { ...state, active: 'a.ts' };
    state = closeFile(state, 'a.ts');
    expect(state.active).toBe('b.ts');
  });

  it('closing an inactive tab does not move focus', () => {
    const state = closeFile(threeOpen(), 'a.ts');
    expect(state.active).toBe('c.ts');
  });

  it('closing the last tab leaves nothing active', () => {
    let state = openFile(emptyEditor, file('a.ts'));
    state = closeFile(state, 'a.ts');
    expect(state).toEqual(emptyEditor);
    expect(activeBuffer(state)).toBeNull();
  });

  it('closing a file that is not open changes nothing', () => {
    const before = threeOpen();
    expect(closeFile(before, 'nowhere.ts')).toEqual(before);
  });
});

describe('following the tree', () => {
  it('a renamed file keeps its buffer and its edits', () => {
    // Without this the tab would point at a path that no longer exists, and its
    // next save would recreate the old file.
    let state = openFile(emptyEditor, file('old.ts', 'body'));
    state = edit(state, 'old.ts', 'body edited');
    state = renamePath(state, 'old.ts', 'new.ts');

    expect(state.active).toBe('new.ts');
    expect(activeBuffer(state)?.current).toBe('body edited');
    expect(dirtyPaths(state)).toEqual(['new.ts']);
  });

  it('deleting a file closes its tab', () => {
    const state = forgetDeleted(threeOpen(), 'b.ts');
    expect(state.buffers.map((buffer) => buffer.path)).toEqual(['a.ts', 'c.ts']);
  });

  it('deleting a directory closes every buffer inside it', () => {
    let state = emptyEditor;
    state = openFile(state, file('src/one.ts'));
    state = openFile(state, file('src/deep/two.ts'));
    state = openFile(state, file('README.md'));

    state = forgetDeleted(state, 'src');
    expect(state.buffers.map((buffer) => buffer.path)).toEqual(['README.md']);
    expect(state.active).toBe('README.md');
  });

  it('a directory named like a prefix of another is not swept up with it', () => {
    // `src2/x.ts` is not inside `src`.
    let state = emptyEditor;
    state = openFile(state, file('src/x.ts'));
    state = openFile(state, file('src2/x.ts'));

    state = forgetDeleted(state, 'src');
    expect(state.buffers.map((buffer) => buffer.path)).toEqual(['src2/x.ts']);
  });

  it('deleting the active file focuses whatever is left', () => {
    const state = forgetDeleted(threeOpen(), 'c.ts');
    expect(state.active).toBe('a.ts');
  });
});

describe('closing several tabs at once', () => {
  it('"close others" keeps one tab and focuses it', () => {
    // Named on a tab that was not active: the request is also a request to
    // look at that file.
    const state = closeOthers(threeOpen(), 'a.ts');
    expect(state.buffers.map((buffer) => buffer.path)).toEqual(['a.ts']);
    expect(state.active).toBe('a.ts');
  });

  it('"close others" on an unknown path changes nothing', () => {
    const before = threeOpen();
    expect(closeOthers(before, 'gone.ts')).toBe(before);
  });

  it('"close to the right" keeps the named tab and everything left of it', () => {
    const state = closeToRight(threeOpen(), 'a.ts');
    expect(state.buffers.map((buffer) => buffer.path)).toEqual(['a.ts']);
    // The active tab was one of the closed ones, so focus falls back.
    expect(state.active).toBe('a.ts');
  });

  it('"close to the right" leaves focus alone when the active tab survives', () => {
    let state = threeOpen();
    state = { ...state, active: 'a.ts' };
    state = closeToRight(state, 'b.ts');
    expect(state.buffers.map((buffer) => buffer.path)).toEqual(['a.ts', 'b.ts']);
    expect(state.active).toBe('a.ts');
  });

  it('"close all" empties the editor', () => {
    expect(closeAll()).toEqual(emptyEditor);
  });
});

describe('paths and labels', () => {
  it('a tab is labelled with the file name', () => {
    expect(tabLabel('src/deep/index.ts')).toBe('index.ts');
    expect(tabLabel('README.md')).toBe('README.md');
  });

  it('the parent of a top-level file is the project root', () => {
    expect(parentOf('README.md')).toBe('');
    expect(parentOf('src/index.ts')).toBe('src');
    expect(parentOf('src/deep/index.ts')).toBe('src/deep');
  });

  it('a child of the root has no leading slash', () => {
    // A leading slash would read as absolute, and the core refuses those.
    expect(childPath('', 'index.ts')).toBe('index.ts');
    expect(childPath('src', 'index.ts')).toBe('src/index.ts');
  });

  it('expanding is a toggle', () => {
    let expanded: string[] = [];
    expanded = toggleExpanded(expanded, 'src');
    expect(expanded).toEqual(['src']);
    expanded = toggleExpanded(expanded, 'src');
    expect(expanded).toEqual([]);
  });
});

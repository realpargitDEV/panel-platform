import { describe, expect, it } from 'vitest';

import {
  describePaste,
  isDescendant,
  nameOf,
  pasteDestination,
  planMove,
  planPaste,
  type Clipboard,
} from './clipboard';

/** `src`, `src/deep` and `docs` are folders; everything else is a file. */
const directories = new Set(['src', 'src/deep', 'docs', 'assets']);
const isDirectory = (path: string) => directories.has(path);

function clipboard(mode: 'copy' | 'cut', paths: string[]): Clipboard {
  return { mode, paths };
}

describe('ancestry', () => {
  it('recognises a path inside another', () => {
    expect(isDescendant('src', 'src/deep/a.ts')).toBe(true);
    expect(isDescendant('src', 'src')).toBe(true);
  });

  it('does not treat a shared prefix as containment', () => {
    // `src` is not an ancestor of `srcfile.ts`.
    expect(isDescendant('src', 'srcfile.ts')).toBe(false);
  });
});

describe('choosing where a paste lands', () => {
  it('uses the one selected folder', () => {
    expect(pasteDestination(['docs'], isDirectory, 'src')).toBe('docs');
  });

  it('uses the current directory when a file is selected', () => {
    expect(pasteDestination(['a.ts'], isDirectory, 'src')).toBe('src');
  });

  it('uses the current directory when several things are selected', () => {
    expect(pasteDestination(['docs', 'assets'], isDirectory, 'src')).toBe('src');
  });

  it('uses the current directory when nothing is selected', () => {
    expect(pasteDestination([], isDirectory, '')).toBe('');
  });
});

describe('planning a copy', () => {
  it('produces one item per clipboard entry', () => {
    const plan = planPaste(clipboard('copy', ['a.ts', 'b.ts']), 'docs', [], isDirectory);
    expect(plan.items).toEqual([
      { from: 'a.ts', to: 'docs/a.ts', isDirectory: false },
      { from: 'b.ts', to: 'docs/b.ts', isDirectory: false },
    ]);
    expect(plan.rejected).toEqual([]);
  });

  it('reports a destination that already exists rather than overwriting it', () => {
    const plan = planPaste(clipboard('copy', ['a.ts']), 'docs', ['docs/a.ts'], isDirectory);
    expect(plan.conflicts).toEqual(['docs/a.ts']);
    // Still planned: the caller asks the user what to do about it.
    expect(plan.items).toHaveLength(1);
  });

  it('treats a difference of case as a conflict', () => {
    // On Windows and a default macOS volume these are the same file, so
    // reporting no conflict would walk the user into an overwrite.
    const plan = planPaste(clipboard('copy', ['docs/README.md']), '', ['readme.md'], isDirectory);
    expect(plan.conflicts).toEqual(['README.md']);
  });

  it('refuses to paste a folder into itself', () => {
    const plan = planPaste(clipboard('copy', ['src']), 'src', [], isDirectory);
    expect(plan.items).toEqual([]);
    expect(plan.rejected[0]?.reason).toContain('cannot be pasted into itself');
  });

  it('refuses to paste a folder into its own descendant', () => {
    // The copy would never finish.
    const plan = planPaste(clipboard('copy', ['src']), 'src/deep', [], isDirectory);
    expect(plan.items).toEqual([]);
    expect(plan.rejected).toHaveLength(1);
  });

  it('refuses two clipboard entries that would land on the same name', () => {
    const plan = planPaste(clipboard('copy', ['src/a.ts', 'docs/a.ts']), '', [], isDirectory);
    expect(plan.items).toHaveLength(1);
    expect(plan.rejected[0]?.reason).toContain('Two of the items');
  });

  it('refuses to copy a file onto itself', () => {
    const plan = planPaste(clipboard('copy', ['docs/a.ts']), 'docs', [], isDirectory);
    expect(plan.items).toEqual([]);
    expect(plan.rejected).toHaveLength(1);
  });
});

describe('planning a cut', () => {
  it('moves items into the destination', () => {
    const plan = planPaste(clipboard('cut', ['src/a.ts']), 'docs', [], isDirectory);
    expect(plan.mode).toBe('cut');
    expect(plan.items).toEqual([{ from: 'src/a.ts', to: 'docs/a.ts', isDirectory: false }]);
  });

  it('says so rather than failing when the item is already there', () => {
    const plan = planPaste(clipboard('cut', ['docs/a.ts']), 'docs', [], isDirectory);
    expect(plan.items).toEqual([]);
    expect(plan.rejected[0]?.reason).toContain('already in this folder');
  });

  it('refuses to move a folder inside itself', () => {
    expect(planPaste(clipboard('cut', ['src']), 'src/deep', [], isDirectory).items).toEqual([]);
  });
});

describe('planning a drag', () => {
  it('follows the same rules as a cut', () => {
    const plan = planMove(['src/a.ts', 'src'], 'src/deep', [], isDirectory);
    expect(plan.items).toEqual([{ from: 'src/a.ts', to: 'src/deep/a.ts', isDirectory: false }]);
    expect(plan.rejected).toHaveLength(1);
  });

  it('detects conflicts before anything moves', () => {
    const plan = planMove(['src/a.ts'], 'docs', ['docs/a.ts'], isDirectory);
    expect(plan.conflicts).toEqual(['docs/a.ts']);
  });
});

describe('describing a plan', () => {
  it('names the operation and the count', () => {
    const copy = planPaste(clipboard('copy', ['a.ts', 'b.ts']), 'docs', [], isDirectory);
    expect(describePaste(copy, 'docs')).toBe('Copying 2 items into docs.');

    const cut = planPaste(clipboard('cut', ['src/a.ts']), 'docs', [], isDirectory);
    expect(describePaste(cut, 'docs')).toBe('Moving 1 item into docs.');
  });

  it('names the root when there is no folder', () => {
    const plan = planPaste(clipboard('copy', ['a.ts']), '', [], isDirectory);
    expect(describePaste(plan, '')).toContain('the project root');
  });

  it('says plainly when a plan does nothing', () => {
    const plan = planPaste(clipboard('copy', ['src']), 'src', [], isDirectory);
    expect(describePaste(plan, 'src')).toContain('Nothing to paste');
  });
});

describe('names', () => {
  it('takes the last component', () => {
    expect(nameOf('src/deep/a.ts')).toBe('a.ts');
    expect(nameOf('a.ts')).toBe('a.ts');
  });
});

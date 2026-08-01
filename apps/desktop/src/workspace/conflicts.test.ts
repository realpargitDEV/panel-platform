import { describe, expect, it } from 'vitest';

import { availablePath, conflictingPaths, resolveConflicts, splitName } from './conflicts';

const existing = ['src/index.ts', 'README.md', 'src/app/main.tsx'];

describe('spotting conflicts', () => {
  it('reports only the paths that are already taken', () => {
    expect(conflictingPaths(existing, ['src/index.ts', 'src/new.ts'])).toEqual(['src/index.ts']);
  });

  it('treats a difference of case as the same file', () => {
    // It is on Windows and on a default macOS volume, and reporting otherwise
    // walks the user into the failure this check exists to avoid.
    expect(conflictingPaths(existing, ['readme.md'])).toEqual(['readme.md']);
  });

  it('reports a repeated conflict once', () => {
    expect(conflictingPaths(existing, ['README.md', 'readme.md'])).toEqual(['README.md']);
  });

  it('finds nothing in an empty project', () => {
    expect(conflictingPaths([], ['a.ts'])).toEqual([]);
  });
});

describe('splitting a name at its extension', () => {
  it('splits an ordinary name', () => {
    expect(splitName('notes.txt')).toEqual({ stem: 'notes', extension: '.txt' });
  });

  it('splits at the last dot', () => {
    expect(splitName('archive.tar.gz')).toEqual({ stem: 'archive.tar', extension: '.gz' });
  });

  it('treats a leading dot as part of the name, not an extension', () => {
    expect(splitName('.gitignore')).toEqual({ stem: '.gitignore', extension: '' });
  });

  it('handles a name with no dot at all', () => {
    expect(splitName('Makefile')).toEqual({ stem: 'Makefile', extension: '' });
  });
});

describe('finding a free name', () => {
  it('returns the path unchanged when nothing is in the way', () => {
    expect(availablePath(existing, 'src/new.ts')).toBe('src/new.ts');
  });

  it('numbers the stem, keeping the extension and the directory', () => {
    expect(availablePath(existing, 'src/index.ts')).toBe('src/index 1.ts');
  });

  it('keeps counting past names that are also taken', () => {
    expect(availablePath([...existing, 'src/index 1.ts'], 'src/index.ts')).toBe('src/index 2.ts');
  });

  it('numbers an extensionless name too', () => {
    expect(availablePath(['Makefile'], 'Makefile')).toBe('Makefile 1');
  });
});

describe('resolving a batch', () => {
  const candidates = [{ path: 'src/index.ts' }, { path: 'src/fresh.ts' }, { path: 'README.md' }];

  it('leaves the files that do not clash alone whatever the choice', () => {
    const { uploads } = resolveConflicts(existing, candidates, 'skip');
    expect(uploads.map((upload) => upload.path)).toEqual(['src/fresh.ts']);
  });

  it('skips the clashing ones and reports them', () => {
    const { skipped } = resolveConflicts(existing, candidates, 'skip');
    expect(skipped.map((item) => item.path)).toEqual(['src/index.ts', 'README.md']);
  });

  it('marks replacements so the caller deletes first', () => {
    const { uploads } = resolveConflicts(existing, candidates, 'replace');
    expect(uploads.filter((upload) => upload.replaces).map((upload) => upload.path)).toEqual([
      'src/index.ts',
      'README.md',
    ]);
  });

  it('renames without ever marking a replacement', () => {
    const { uploads, skipped } = resolveConflicts(existing, candidates, 'rename');
    expect(uploads.map((upload) => upload.path)).toEqual([
      'src/index 1.ts',
      'src/fresh.ts',
      'README 1.md',
    ]);
    expect(uploads.every((upload) => !upload.replaces)).toBe(true);
    expect(skipped).toEqual([]);
  });

  it('gives two copies of one name two different names', () => {
    // Both clash with the same existing file; resolving each against the disk
    // alone would hand them both the same replacement.
    const { uploads } = resolveConflicts(
      existing,
      [{ path: 'README.md' }, { path: 'README.md' }],
      'rename',
    );
    expect(uploads.map((upload) => upload.path)).toEqual(['README 1.md', 'README 2.md']);
  });

  it('notices a batch claiming the same new name twice', () => {
    const { uploads } = resolveConflicts([], [{ path: 'new.ts' }, { path: 'new.ts' }], 'rename');
    expect(uploads.map((upload) => upload.path)).toEqual(['new.ts', 'new 1.ts']);
  });
});

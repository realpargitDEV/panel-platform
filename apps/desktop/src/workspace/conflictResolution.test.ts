import { describe, expect, it } from 'vitest';

import {
  allConflicts,
  allowedFor,
  analyse,
  applyToAll,
  conflictKind,
  defaultDecisions,
  describeConflict,
  generateName,
  normalisePath,
  planIsStale,
  resolvePlan,
  samePath,
  setDecision,
  unresolvedCount,
  type ItemKind,
  type PlannedItem,
} from './conflictResolution';

function item(source: string, destination: string, incoming: ItemKind = 'file'): PlannedItem {
  return { source, destination, incoming };
}

function disk(entries: Record<string, ItemKind>): Map<string, ItemKind> {
  return new Map(Object.entries(entries));
}

describe('normalising paths', () => {
  it('compares case-insensitively, as Windows does', () => {
    // Treating these as distinct is how an overwrite happens with no conflict.
    expect(samePath('README.md', 'readme.md')).toBe(true);
  });

  it('treats both separators as the same', () => {
    expect(samePath('src\\a.ts', 'src/a.ts')).toBe(true);
  });

  it('ignores a trailing separator', () => {
    expect(normalisePath('src/')).toBe('src');
  });
});

describe('classifying a collision', () => {
  it('names each combination', () => {
    expect(conflictKind('file', 'file')).toBe('file-over-file');
    expect(conflictKind('directory', 'directory')).toBe('directory-over-directory');
    expect(conflictKind('file', 'directory')).toBe('file-over-directory');
    expect(conflictKind('directory', 'file')).toBe('directory-over-file');
  });

  it('offers merge only for two directories', () => {
    expect(allowedFor('directory-over-directory')).toContain('merge');
    expect(allowedFor('file-over-file')).not.toContain('merge');
    expect(allowedFor('file-over-directory')).not.toContain('merge');
    expect(allowedFor('directory-over-file')).not.toContain('merge');
  });
});

describe('generating a free name', () => {
  it('returns the path untouched when nothing is in the way', () => {
    expect(generateName('config.json', [])).toBe('config.json');
  });

  it('adds "copy", then numbers it', () => {
    expect(generateName('config.json', ['config.json'])).toBe('config copy.json');
    expect(generateName('config.json', ['config.json', 'config copy.json'])).toBe(
      'config copy 2.json',
    );
  });

  it('has no extension to preserve for a folder', () => {
    expect(generateName('src', ['src'])).toBe('src copy');
    expect(generateName('src', ['src', 'src copy'])).toBe('src copy 2');
  });

  it('keeps the directory it was going into', () => {
    expect(generateName('docs/config.json', ['docs/config.json'])).toBe('docs/config copy.json');
  });

  it('treats a leading dot as part of the name, not an extension', () => {
    expect(generateName('.env', ['.env'])).toBe('.env copy');
  });

  it('avoids a name taken in a different case', () => {
    expect(generateName('Config.json', ['config.json'])).toBe('Config copy.json');
  });
});

describe('analysing a batch', () => {
  it('separates what collides from what does not', () => {
    const analysis = analyse(
      [item('a/x.ts', 'x.ts'), item('a/y.ts', 'y.ts')],
      disk({ 'x.ts': 'file' }),
      'copy',
    );
    expect(analysis.conflicts.map((conflict) => conflict.destination)).toEqual(['x.ts']);
    expect(analysis.clear.map((entry) => entry.destination)).toEqual(['y.ts']);
  });

  it('reports the kind of both sides', () => {
    const analysis = analyse([item('a/src', 'src', 'directory')], disk({ src: 'file' }), 'move');
    expect(analysis.conflicts[0]?.kind).toBe('directory-over-file');
    expect(analysis.conflicts[0]?.existing).toBe('file');
    expect(analysis.conflicts[0]?.incoming).toBe('directory');
  });

  it('detects two incoming items wanting one destination', () => {
    // Not the disk's fault, and still has to be resolved.
    const analysis = analyse([item('a/x.ts', 'x.ts'), item('b/x.ts', 'x.ts')], disk({}), 'copy');
    expect(analysis.clear).toHaveLength(1);
    expect(analysis.internal).toHaveLength(1);
    expect(analysis.internal[0]?.allowed).toEqual(['rename', 'skip']);
  });

  it('spots a collision that differs only in case', () => {
    const analysis = analyse(
      [item('a/README.md', 'README.md')],
      disk({ 'readme.md': 'file' }),
      'copy',
    );
    expect(analysis.conflicts).toHaveLength(1);
  });

  it('carries the operation through, so the dialog can name it', () => {
    const analysis = analyse([item('a/x.ts', 'x.ts')], disk({ 'x.ts': 'file' }), 'import');
    expect(analysis.conflicts[0]?.operation).toBe('import');
  });
});

describe('decisions', () => {
  const analysis = analyse(
    [item('a/x.ts', 'x.ts'), item('a/src', 'src', 'directory'), item('a/y.ts', 'y.ts')],
    disk({ 'x.ts': 'file', src: 'directory' }),
    'copy',
  );
  const conflicts = allConflicts(analysis);

  it('defaults two folders to merging and everything else to unresolved', () => {
    const decisions = defaultDecisions(conflicts);
    expect(decisions['disk:a/src']?.resolution).toBe('merge');
    expect(decisions['disk:a/x.ts']?.resolution).toBe('unresolved');
  });

  it('refuses to confirm while anything is unresolved', () => {
    expect(unresolvedCount(conflicts, defaultDecisions(conflicts))).toBe(1);
  });

  it('is settled once every conflict has an answer', () => {
    const decisions = setDecision(defaultDecisions(conflicts), 'disk:a/x.ts', 'skip');
    expect(unresolvedCount(conflicts, decisions)).toBe(0);
  });

  it('applies one answer to every conflict', () => {
    const decisions = applyToAll(conflicts, defaultDecisions(conflicts), 'rename');
    expect(unresolvedCount(conflicts, decisions)).toBe(0);
  });

  it('applies an answer only to conflicts of one kind', () => {
    const decisions = applyToAll(
      conflicts,
      defaultDecisions(conflicts),
      'replace',
      'file-over-file',
    );
    expect(decisions['disk:a/x.ts']?.resolution).toBe('replace');
    // The folder keeps its merge rather than being replaced wholesale.
    expect(decisions['disk:a/src']?.resolution).toBe('merge');
  });

  it('never applies an answer a conflict does not allow', () => {
    const fileConflicts = allConflicts(
      analyse([item('a/x.ts', 'x.ts')], disk({ 'x.ts': 'file' }), 'copy'),
    );
    const decisions = applyToAll(fileConflicts, defaultDecisions(fileConflicts), 'merge');
    // Merge is meaningless for two files, so it was ignored.
    expect(decisions['disk:a/x.ts']?.resolution).toBe('unresolved');
  });
});

describe('resolving into work', () => {
  const analysis = analyse(
    [item('a/x.ts', 'x.ts'), item('a/y.ts', 'y.ts')],
    disk({ 'x.ts': 'file' }),
    'copy',
  );

  it('carries the clear items through untouched', () => {
    const plan = resolvePlan(analysis, defaultDecisions(allConflicts(analysis)), ['x.ts']);
    expect(plan.items.some((entry) => entry.destination === 'y.ts')).toBe(true);
  });

  it('marks a replacement so the caller stages a restore', () => {
    const decisions = setDecision(
      defaultDecisions(allConflicts(analysis)),
      'disk:a/x.ts',
      'replace',
    );
    const plan = resolvePlan(analysis, decisions, ['x.ts']);
    expect(plan.items.find((entry) => entry.source === 'a/x.ts')?.replaces).toBe(true);
  });

  it('renames around the collision and records the final path', () => {
    const decisions = setDecision(
      defaultDecisions(allConflicts(analysis)),
      'disk:a/x.ts',
      'rename',
    );
    const plan = resolvePlan(analysis, decisions, ['x.ts']);
    expect(plan.items.find((entry) => entry.source === 'a/x.ts')?.destination).toBe('x copy.ts');
    expect(plan.renames['disk:a/x.ts']).toBe('x copy.ts');
  });

  it('leaves a skipped item out of the work entirely', () => {
    const decisions = setDecision(defaultDecisions(allConflicts(analysis)), 'disk:a/x.ts', 'skip');
    const plan = resolvePlan(analysis, decisions, ['x.ts']);
    expect(plan.items.some((entry) => entry.source === 'a/x.ts')).toBe(false);
    expect(plan.skipped).toContain('a/x.ts');
  });

  it('never carries out an unresolved conflict', () => {
    const plan = resolvePlan(analysis, defaultDecisions(allConflicts(analysis)), ['x.ts']);
    expect(plan.items.some((entry) => entry.source === 'a/x.ts')).toBe(false);
  });

  it('gives two renamed items two different names', () => {
    const twin = analyse(
      [item('a/x.ts', 'x.ts'), item('b/x.ts', 'x.ts')],
      disk({ 'x.ts': 'file' }),
      'copy',
    );
    const decisions = applyToAll(
      allConflicts(twin),
      defaultDecisions(allConflicts(twin)),
      'rename',
    );
    const plan = resolvePlan(twin, decisions, ['x.ts']);
    const destinations = plan.items.map((entry) => entry.destination);
    expect(new Set(destinations).size).toBe(destinations.length);
  });

  it('marks a merge without marking it a replacement', () => {
    const folders = analyse(
      [item('a/src', 'src', 'directory')],
      disk({ src: 'directory' }),
      'copy',
    );
    const plan = resolvePlan(folders, defaultDecisions(allConflicts(folders)), ['src']);
    const entry = plan.items[0];
    expect(entry?.merges).toBe(true);
    expect(entry?.replaces).toBe(false);
  });
});

describe('staleness', () => {
  const analysis = analyse([item('a/y.ts', 'y.ts')], disk({}), 'copy');
  const plan = resolvePlan(analysis, {}, []);

  it('is fresh when the destination has not changed', () => {
    expect(planIsStale(plan, [])).toBe(false);
  });

  it('is stale once something appears where a clear item was going', () => {
    // Otherwise the file that arrived while the dialog was open is overwritten.
    expect(planIsStale(plan, ['y.ts'])).toBe(true);
  });

  it('does not call a plan stale for a path it already agreed to replace', () => {
    const conflict = analyse([item('a/x.ts', 'x.ts')], disk({ 'x.ts': 'file' }), 'copy');
    const decisions = setDecision(
      defaultDecisions(allConflicts(conflict)),
      'disk:a/x.ts',
      'replace',
    );
    const replacing = resolvePlan(conflict, decisions, ['x.ts']);
    expect(planIsStale(replacing, ['x.ts'])).toBe(false);
  });
});

describe('wording', () => {
  it('explains a folder merge', () => {
    const analysis = analyse(
      [item('a/src', 'src', 'directory')],
      disk({ src: 'directory' }),
      'copy',
    );
    expect(describeConflict(analysis.conflicts[0]!)).toContain('Merging');
  });

  it('says plainly when two kinds cannot be combined', () => {
    const analysis = analyse([item('a/src', 'src', 'directory')], disk({ src: 'file' }), 'copy');
    expect(describeConflict(analysis.conflicts[0]!)).toContain('cannot be combined');
  });
});

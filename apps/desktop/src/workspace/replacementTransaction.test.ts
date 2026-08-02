/**
 * The replacement transaction, driven against a filesystem that fails on cue.
 *
 * Rollback is code that only runs after something has already gone wrong, so
 * the only honest way to test it is to make the failure happen deliberately.
 * The fake filesystem here is a real model — it holds files, renames really
 * move them, deletes really remove them — so an assertion like "the original
 * came back" is checking the contents of a directory, not a call log.
 *
 * What these prove, in the order the requirement asks for it:
 *   - existing files are staged *before* anything is copied
 *   - the replaced original is deleted only after the copy has committed
 *   - a forced failure puts the original back
 *   - a failure in a middle batch restores every batch's replacements
 *   - files this operation created are removed on rollback
 *   - files it did not touch are still there afterwards
 *   - a rollback that cannot finish names the exact paths
 */
import { describe, expect, it } from 'vitest';

import type { OperationBatch } from './importOperation';
import { runReplacementTransaction, type TransactionIo } from './replacementTransaction';
import { groupsFrom, planFrom, setDestination } from './importGroups';
import type { ImportCandidate } from '../api';

// ------------------------------------------------------------ fake filesystem

/**
 * A project directory, as a map of path to contents.
 *
 * `directories` is kept separately so a folder can exist while empty, which is
 * what an import of an empty folder produces and what a rollback has to remove.
 */
class FakeProject {
  files = new Map<string, string>();
  directories = new Set<string>();

  /** Every operation is recorded, so ordering can be asserted. */
  log: string[] = [];

  /** Deterministic failure injection: return a message to make it throw. */
  failOn: (operation: string, path: string) => string | null = () => null;

  constructor(initial: Record<string, string> = {}) {
    for (const [path, contents] of Object.entries(initial)) this.files.set(path, contents);
  }

  private refuse(operation: string, path: string) {
    const message = this.failOn(operation, path);
    if (message !== null) throw new Error(message);
  }

  private parentOf(path: string): string {
    const cut = path.lastIndexOf('/');
    return cut < 0 ? '' : path.slice(0, cut);
  }

  io(): TransactionIo {
    return {
      rename: async (path, toName) => {
        this.log.push(`rename ${path} -> ${toName}`);
        this.refuse('rename', path);
        const target = this.parentOf(path) ? `${this.parentOf(path)}/${toName}` : toName;
        if (this.files.has(path)) {
          this.files.set(target, this.files.get(path)!);
          this.files.delete(path);
        } else if (this.directories.has(path)) {
          this.directories.delete(path);
          this.directories.add(target);
        } else {
          throw new Error(`${path} does not exist`);
        }
      },
      remove: async (path, isDirectory) => {
        this.log.push(`remove ${path}`);
        this.refuse('remove', path);
        if (isDirectory) this.directories.delete(path);
        this.files.delete(path);
        for (const key of [...this.files.keys()]) {
          if (key.startsWith(`${path}/`)) this.files.delete(key);
        }
      },
      importBatch: async (batch) => {
        this.log.push(`import ${batch.groupName}`);
        this.refuse('import', batch.groupName);
        const created: string[] = [];
        for (const [, name] of batch.destinationNames.length > 0
          ? batch.destinationNames
          : batch.sourcePaths.map((source): [string, string] => [
              source,
              source.split('/').pop() ?? source,
            ])) {
          const path = batch.destination ? `${batch.destination}/${name}` : name;
          this.files.set(path, `imported:${name}`);
          created.push(path);
        }
        return created;
      },
      isDirectory: (path) => this.directories.has(path),
    };
  }
}

function batch(overrides: Partial<OperationBatch> & { groupName: string }): OperationBatch {
  return {
    importId: `import-${overrides.groupName}`,
    destination: '',
    sourcePaths: [],
    unwrapPaths: [],
    destinationNames: [],
    replacePaths: [],
    totalEntries: 1,
    totalBytes: 10,
    ...overrides,
  };
}

// --------------------------------------------------------------------- tests

describe('staging before replacing', () => {
  it('moves the existing file aside before a single byte is copied', async () => {
    const project = new FakeProject({ 'app.ts': 'original' });
    await runReplacementTransaction(
      [
        batch({
          groupName: 'Dashboard',
          sourcePaths: ['/w/app.ts'],
          replacePaths: ['app.ts'],
        }),
      ],
      project.io(),
    );

    const renameAt = project.log.findIndex((entry) => entry.startsWith('rename app.ts'));
    const importAt = project.log.findIndex((entry) => entry.startsWith('import'));
    expect(renameAt).toBeGreaterThanOrEqual(0);
    expect(renameAt).toBeLessThan(importAt);
  });

  it('commits the replacement content and only then deletes the original', async () => {
    const project = new FakeProject({ 'app.ts': 'original' });
    const result = await runReplacementTransaction(
      [batch({ groupName: 'Dashboard', sourcePaths: ['/w/app.ts'], replacePaths: ['app.ts'] })],
      project.io(),
    );

    expect(result.outcome).toBe('completed');
    const importAt = project.log.findIndex((entry) => entry.startsWith('import'));
    const deleteAt = project.log.findIndex((entry) => entry.startsWith('remove app.ts.replaced'));
    expect(deleteAt).toBeGreaterThan(importAt);

    // The new content is in place and the backup is gone.
    expect(project.files.get('app.ts')).toBe('imported:app.ts');
    expect([...project.files.keys()].filter((path) => path.includes('.replaced'))).toEqual([]);
  });

  it('does not copy anything when the staging rename itself fails', async () => {
    const project = new FakeProject({ 'app.ts': 'original' });
    project.failOn = (operation) => (operation === 'rename' ? 'locked by another process' : null);

    const result = await runReplacementTransaction(
      [batch({ groupName: 'Dashboard', sourcePaths: ['/w/app.ts'], replacePaths: ['app.ts'] })],
      project.io(),
    );

    expect(result.outcome).toBe('failed');
    expect(result.failure).toMatch(/app\.ts: locked by another process/);
    expect(project.log.some((entry) => entry.startsWith('import'))).toBe(false);
    // Nothing was staged, so the original is exactly where it was.
    expect(project.files.get('app.ts')).toBe('original');
  });
});

describe('restoring after a forced failure', () => {
  it('puts the original back when the copy fails', async () => {
    const project = new FakeProject({ 'app.ts': 'original' });
    project.failOn = (operation) => (operation === 'import' ? 'the disk filled up' : null);

    const result = await runReplacementTransaction(
      [batch({ groupName: 'Dashboard', sourcePaths: ['/w/app.ts'], replacePaths: ['app.ts'] })],
      project.io(),
    );

    expect(result.outcome).toBe('failed');
    expect(project.files.get('app.ts')).toBe('original');
    expect([...project.files.keys()]).toEqual(['app.ts']);
  });

  it('removes the files it created during the rollback', async () => {
    const project = new FakeProject({ 'keep.txt': 'untouched' });
    // The first batch succeeds; the second fails, so the first must be undone.
    project.failOn = (operation, path) =>
      operation === 'import' && path === 'Second' ? 'refused' : null;

    const result = await runReplacementTransaction(
      [
        batch({ groupName: 'First', sourcePaths: ['/w/one.ts'] }),
        batch({ groupName: 'Second', sourcePaths: ['/w/two.ts'] }),
      ],
      project.io(),
    );

    expect(result.outcome).toBe('failed');
    expect(project.files.has('one.ts')).toBe(false);
    expect(project.files.get('keep.txt')).toBe('untouched');
  });

  it('leaves files it never touched exactly as they were', async () => {
    const project = new FakeProject({
      'app.ts': 'original',
      'untouched.md': 'mine',
      'also/untouched.txt': 'mine too',
    });
    project.failOn = (operation) => (operation === 'import' ? 'refused' : null);

    await runReplacementTransaction(
      [batch({ groupName: 'Dashboard', sourcePaths: ['/w/app.ts'], replacePaths: ['app.ts'] })],
      project.io(),
    );

    expect(project.files.get('untouched.md')).toBe('mine');
    expect(project.files.get('also/untouched.txt')).toBe('mine too');
    expect(project.files.get('app.ts')).toBe('original');
  });

  it('restores every replaced file when a middle batch of a multi-group import fails', async () => {
    const project = new FakeProject({
      'one.ts': 'first original',
      'lib/two.ts': 'second original',
      'three.ts': 'third original',
    });
    project.failOn = (operation, path) =>
      operation === 'import' && path === 'Third' ? 'refused partway through' : null;

    const result = await runReplacementTransaction(
      [
        batch({ groupName: 'First', sourcePaths: ['/w/one.ts'], replacePaths: ['one.ts'] }),
        batch({
          groupName: 'Second',
          destination: 'lib',
          sourcePaths: ['/w/two.ts'],
          replacePaths: ['lib/two.ts'],
        }),
        batch({ groupName: 'Third', sourcePaths: ['/w/three.ts'], replacePaths: ['three.ts'] }),
      ],
      project.io(),
    );

    expect(result.outcome).toBe('failed');
    // Every original is back, including the ones from batches that had already
    // succeeded before the failing one.
    expect(project.files.get('one.ts')).toBe('first original');
    expect(project.files.get('lib/two.ts')).toBe('second original');
    expect(project.files.get('three.ts')).toBe('third original');
    // And nothing is left lying around under a backup name.
    expect([...project.files.keys()].filter((path) => path.includes('.replaced'))).toEqual([]);
  });

  it('undoes a cancellation the same way as a failure', async () => {
    const project = new FakeProject({ 'app.ts': 'original' });
    let cancelled = false;

    const result = await runReplacementTransaction(
      [
        batch({ groupName: 'First', sourcePaths: ['/w/one.ts'], replacePaths: ['app.ts'] }),
        batch({ groupName: 'Second', sourcePaths: ['/w/two.ts'] }),
      ],
      project.io(),
      {
        onBatchComplete: () => {
          cancelled = true;
        },
        isCancelled: () => cancelled,
      },
    );

    expect(result.outcome).toBe('cancelled');
    expect(project.files.get('app.ts')).toBe('original');
  });
});

describe('reporting a rollback that could not finish', () => {
  it('names the exact path and where the original is still sitting', async () => {
    const project = new FakeProject({ 'app.ts': 'original' });
    let staged = false;
    project.failOn = (operation, path) => {
      if (operation === 'import') return 'refused';
      // Let the staging rename through, then refuse the one that puts it back.
      if (operation === 'rename' && staged) return 'still locked';
      if (operation === 'rename' && path === 'app.ts') {
        staged = true;
        return null;
      }
      return null;
    };

    const result = await runReplacementTransaction(
      [batch({ groupName: 'Dashboard', sourcePaths: ['/w/app.ts'], replacePaths: ['app.ts'] })],
      project.io(),
    );

    expect(result.outcome).toBe('failed');
    const rollbackErrors = result.errors.filter((error) => error.rollback === true);
    expect(rollbackErrors).toHaveLength(1);
    expect(rollbackErrors[0]?.path).toBe('app.ts');
    // The message has to say where the file actually is, because the user has
    // to go and find it.
    expect(rollbackErrors[0]?.message).toContain('app.ts.replaced');
    expect(rollbackErrors[0]?.message).toContain('still locked');
  });

  it('names a created file it could not remove', async () => {
    const project = new FakeProject();
    project.failOn = (operation, path) => {
      if (operation === 'import' && path === 'Second') return 'refused';
      if (operation === 'remove') return 'permission denied';
      return null;
    };

    const result = await runReplacementTransaction(
      [
        batch({ groupName: 'First', sourcePaths: ['/w/one.ts'] }),
        batch({ groupName: 'Second', sourcePaths: ['/w/two.ts'] }),
      ],
      project.io(),
    );

    const failures = result.errors.filter((error) => error.rollback === true);
    expect(failures[0]?.path).toBe('one.ts');
    expect(failures[0]?.message).toMatch(/could not be removed: permission denied/);
  });

  it('reports a leftover backup without calling the import a failure', async () => {
    const project = new FakeProject({ 'app.ts': 'original' });
    project.failOn = (operation) => (operation === 'remove' ? 'in use' : null);

    const result = await runReplacementTransaction(
      [batch({ groupName: 'Dashboard', sourcePaths: ['/w/app.ts'], replacePaths: ['app.ts'] })],
      project.io(),
    );

    // The import worked; the tidy-up did not. Those are different outcomes.
    expect(result.outcome).toBe('completed');
    expect(result.errors).toHaveLength(1);
    expect(result.errors[0]?.rollback).toBeUndefined();
    expect(result.errors[0]?.path).toContain('.replaced');
  });
});

describe('the phases the dialog is driven by', () => {
  it('reports staging, copying and committing, and never a moving-sources phase', async () => {
    const project = new FakeProject({ 'app.ts': 'original' });
    const phases: string[] = [];

    await runReplacementTransaction(
      [batch({ groupName: 'Dashboard', sourcePaths: ['/w/app.ts'], replacePaths: ['app.ts'] })],
      project.io(),
      { onPhase: (phase) => phases.push(phase) },
    );

    expect(phases).toEqual(['staging-replacements', 'copying', 'committing']);
  });

  it('does not report staging for a batch that replaces nothing', async () => {
    const project = new FakeProject();
    const phases: string[] = [];

    await runReplacementTransaction(
      [batch({ groupName: 'Dashboard', sourcePaths: ['/w/app.ts'] })],
      project.io(),
      { onPhase: (phase) => phases.push(phase) },
    );

    expect(phases).not.toContain('staging-replacements');
  });

  it('counts the rollback so the bar can show it', async () => {
    const project = new FakeProject({ 'app.ts': 'original' });
    project.failOn = (operation, path) =>
      operation === 'import' && path === 'Second' ? 'refused' : null;

    let total = 0;
    let steps = 0;
    await runReplacementTransaction(
      [
        batch({ groupName: 'First', sourcePaths: ['/w/one.ts'], replacePaths: ['app.ts'] }),
        batch({ groupName: 'Second', sourcePaths: ['/w/two.ts'] }),
      ],
      project.io(),
      {
        onRollbackStart: (count) => {
          total = count;
        },
        onRollbackStep: () => {
          steps += 1;
        },
      },
    );

    // One created file to remove, one staged file to put back.
    expect(total).toBe(2);
    expect(steps).toBe(2);
  });
});

describe('a destination chosen in the dialog reaches the transaction', () => {
  it('carries the relocated group through to the batch that is executed', async () => {
    const dashboard: ImportCandidate = {
      path: '/w/Dashboard',
      name: 'Dashboard',
      isDirectory: true,
      isProject: false,
      score: 0,
      signals: [],
      children: [],
      childCount: 0,
      ecosystem: null,
      isMonorepo: false,
      nested: [],
    };

    // The organiser plan after "choose another destination" picked
    // `Projects/NewDashboard` for the group.
    const groups = setDestination(groupsFrom([dashboard], ''), '/w/Dashboard', 'Projects/New');
    const plan = planFrom(groups);

    const project = new FakeProject();
    const result = await runReplacementTransaction(
      plan.batches.map((entry) =>
        batch({
          groupName: 'Dashboard',
          destination: entry.destination,
          sourcePaths: entry.sourcePaths,
          unwrapPaths: entry.unwrapPaths,
        }),
      ),
      project.io(),
    );

    expect(result.outcome).toBe('completed');
    // The file landed under the destination chosen in the dialog, not the one
    // the organiser originally had.
    expect(result.created).toEqual(['Projects/New/Dashboard']);
    expect(project.files.has('Projects/New/Dashboard')).toBe(true);
  });
});

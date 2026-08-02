/**
 * Relocation is the one resolution that changes where things land, so the
 * things worth pinning down are the ones a screenshot cannot show: that a
 * group's tree survives the move, that a decision made about one collision is
 * not silently reused for a different one, and that every way of putting a
 * folder inside itself is refused.
 */
import { describe, expect, it } from 'vitest';

import {
  allConflicts,
  analyse,
  type Conflict,
  type Decisions,
  type ItemKind,
  type PlannedItem,
} from './conflictResolution';
import {
  baseNameOf,
  isAtOrInside,
  isInside,
  joinRelative,
  noGrouping,
  parentOf,
  pickableDirectories,
  planRelocation,
  preserveDecisions,
  previewFinalPath,
  rebase,
  targetsOf,
  validateDestinationPath,
  validateRelocation,
  type PlanGrouping,
  type RelocationContext,
} from './relocation';

function conflictFor(
  source: string,
  destination: string,
  incoming: ItemKind = 'file',
  existing: ItemKind = 'file',
): Conflict {
  return {
    id: `disk:${source}`,
    source,
    destination,
    kind:
      incoming === 'directory' && existing === 'directory'
        ? 'directory-over-directory'
        : 'file-over-file',
    incoming,
    existing,
    operation: 'import',
    allowed: ['replace', 'rename', 'skip'],
    renameable: true,
  };
}

function contextOf(
  items: PlannedItem[],
  existing: [string, ItemKind][] = [],
  grouping: PlanGrouping = noGrouping,
): RelocationContext {
  return { items, existing: new Map(existing), grouping };
}

describe('path helpers', () => {
  it('splits a relative path into its parent and its name', () => {
    expect(parentOf('a/b/c.txt')).toBe('a/b');
    expect(baseNameOf('a/b/c.txt')).toBe('c.txt');
    expect(parentOf('c.txt')).toBe('');
    expect(baseNameOf('c.txt')).toBe('c.txt');
  });

  it('treats the project root as the empty path rather than a slash', () => {
    expect(joinRelative('', 'a.txt')).toBe('a.txt');
    expect(joinRelative('src', 'a.txt')).toBe('src/a.txt');
  });

  it('knows when a path is inside another, case-insensitively', () => {
    expect(isAtOrInside('Src/app.ts', 'src')).toBe(true);
    expect(isAtOrInside('src', 'src')).toBe(true);
    expect(isInside('src', 'src')).toBe(false);
    expect(isAtOrInside('srcish/app.ts', 'src')).toBe(false);
  });

  it('counts everything as inside the project root', () => {
    expect(isAtOrInside('anything/at/all', '')).toBe(true);
  });
});

describe('rebase', () => {
  it('keeps a child at its own depth under the new root', () => {
    expect(rebase('Dashboard/src/app.ts', 'Dashboard', 'Projects/NewDashboard')).toBe(
      'Projects/NewDashboard/src/app.ts',
    );
  });

  it('moves the root itself', () => {
    expect(rebase('Dashboard', 'Dashboard', 'Projects/NewDashboard')).toBe('Projects/NewDashboard');
  });

  it('treats an empty root as "already relative", which is what an unwrapped group is', () => {
    expect(rebase('package.json', '', 'Projects/NewDashboard')).toBe(
      'Projects/NewDashboard/package.json',
    );
    expect(rebase('src/app.ts', '', 'Projects/NewDashboard')).toBe(
      'Projects/NewDashboard/src/app.ts',
    );
  });

  it('refuses a path that was never under the root', () => {
    expect(rebase('Other/app.ts', 'Dashboard', 'Projects/NewDashboard')).toBeNull();
  });

  it('never rebuilds a path from the file name alone', () => {
    const moved = rebase('Dashboard/src/deep/nested/app.ts', 'Dashboard', 'Projects/New');
    expect(moved).toBe('Projects/New/src/deep/nested/app.ts');
    expect(moved).not.toBe('Projects/New/app.ts');
  });
});

describe('validateDestinationPath', () => {
  it('refuses an empty destination', () => {
    expect(validateDestinationPath('')?.message).toMatch(/Enter a destination/);
    expect(validateDestinationPath('   ')?.message).toMatch(/Enter a destination/);
  });

  it('refuses an absolute path on either platform', () => {
    expect(validateDestinationPath('/etc')?.message).toMatch(/inside the project/);
    expect(validateDestinationPath('C:\\Windows')?.message).toMatch(/inside the project/);
  });

  it('refuses relative traversal out of the project', () => {
    expect(validateDestinationPath('../outside')?.message).toMatch(/\.\./);
    expect(validateDestinationPath('src/../../outside')?.message).toMatch(/\.\./);
  });

  it('refuses characters a folder name cannot hold', () => {
    expect(validateDestinationPath('a<b')?.message).toMatch(/cannot have/);
    expect(validateDestinationPath('a?b')?.message).toMatch(/cannot have/);
  });

  it('refuses names Windows reserves', () => {
    expect(validateDestinationPath('con')?.message).toMatch(/reserved/);
    expect(validateDestinationPath('src/NUL')?.message).toMatch(/reserved/);
  });

  it('refuses a segment ending in a space or a dot', () => {
    expect(validateDestinationPath('src/name /deep')?.message).toMatch(/space or a dot/);
    expect(validateDestinationPath('src/name.')?.message).toMatch(/space or a dot/);
  });

  it('forgives whitespace around the whole path, which is a typing artefact', () => {
    expect(validateDestinationPath('  src/name  ')).toBeNull();
  });

  it('accepts ordinary folder names, including spaces and dashes', () => {
    expect(validateDestinationPath('Projects/My New-Dashboard')).toBeNull();
    expect(validateDestinationPath('src')).toBeNull();
  });
});

describe('validateRelocation', () => {
  it('refuses a destination that is a file', () => {
    const conflict = conflictFor('/src/a.txt', 'a.txt');
    const context = contextOf(
      [{ source: '/src/a.txt', destination: 'a.txt', incoming: 'file' }],
      [['notes.md', 'file']],
    );
    expect(validateRelocation(conflict, 'notes.md', 'one', context)?.message).toMatch(/is a file/);
  });

  it('refuses moving a folder into itself', () => {
    const conflict = conflictFor('/w/Dashboard', 'Dashboard', 'directory', 'directory');
    const context = contextOf([
      { source: '/w/Dashboard', destination: 'Dashboard', incoming: 'directory' },
    ]);
    expect(validateRelocation(conflict, 'Dashboard', 'one', context)?.message).toMatch(
      /into itself/,
    );
  });

  it('refuses moving a folder into one of its own descendants', () => {
    const conflict = conflictFor('/w/Dashboard', 'Dashboard', 'directory', 'directory');
    const context = contextOf([
      { source: '/w/Dashboard', destination: 'Dashboard', incoming: 'directory' },
    ]);
    expect(validateRelocation(conflict, 'Dashboard/src/deep', 'one', context)?.message).toMatch(
      /its own folders/,
    );
  });

  it('refuses a destination two incoming entries would both land in', () => {
    const conflict = conflictFor('/a/notes.md', 'notes.md');
    const context = contextOf([
      { source: '/a/notes.md', destination: 'notes.md', incoming: 'file' },
      { source: '/b/notes.md', destination: 'archive/notes.md', incoming: 'file' },
    ]);
    expect(validateRelocation(conflict, 'archive', 'one', context)?.message).toMatch(
      /already lands at/,
    );
  });

  it('refuses the directory the item is already in', () => {
    const conflict = conflictFor('/a/notes.md', 'docs/notes.md');
    const context = contextOf([
      { source: '/a/notes.md', destination: 'docs/notes.md', incoming: 'file' },
    ]);
    expect(validateRelocation(conflict, 'docs', 'one', context)?.message).toMatch(/already is/);
  });

  it('accepts a free folder elsewhere in the project', () => {
    const conflict = conflictFor('/a/notes.md', 'notes.md');
    const context = contextOf([
      { source: '/a/notes.md', destination: 'notes.md', incoming: 'file' },
    ]);
    expect(validateRelocation(conflict, 'archive', 'one', context)).toBeNull();
  });
});

describe('targetsOf', () => {
  const conflicts = [
    conflictFor('/w/Dashboard/package.json', 'package.json'),
    conflictFor('/w/Dashboard/src/app.ts', 'src/app.ts'),
    conflictFor('/w/notes.md', 'notes.md'),
  ];
  const grouping: PlanGrouping = {
    groupOf: {
      '/w/Dashboard/package.json': 'dash',
      '/w/Dashboard/src/app.ts': 'dash',
      '/w/notes.md': 'loose',
    },
    rootOf: { dash: '', loose: '' },
  };

  it('scopes to one conflict', () => {
    const ids = targetsOf(
      conflicts,
      { conflictId: 'disk:/w/notes.md', destination: 'archive', scope: 'one' },
      grouping,
    );
    expect(ids).toEqual(['disk:/w/notes.md']);
  });

  it('scopes to every conflict in the same incoming group', () => {
    const ids = targetsOf(
      conflicts,
      { conflictId: 'disk:/w/Dashboard/package.json', destination: 'Projects', scope: 'group' },
      grouping,
    );
    expect(ids).toEqual(['disk:/w/Dashboard/package.json', 'disk:/w/Dashboard/src/app.ts']);
  });

  it('scopes to every conflict heading for the same directory', () => {
    const ids = targetsOf(
      conflicts,
      {
        conflictId: 'disk:/w/Dashboard/package.json',
        destination: 'archive',
        scope: 'same-destination',
      },
      grouping,
    );
    // `package.json` and `notes.md` are both at the root; `src/app.ts` is not.
    expect(ids).toEqual(['disk:/w/Dashboard/package.json', 'disk:/w/notes.md']);
  });

  it('scopes to the whole batch', () => {
    const ids = targetsOf(
      conflicts,
      { conflictId: 'disk:/w/notes.md', destination: 'archive', scope: 'all' },
      grouping,
    );
    expect(ids).toHaveLength(3);
  });

  it('falls back to the single conflict when there are no groups', () => {
    const ids = targetsOf(
      conflicts,
      { conflictId: 'disk:/w/notes.md', destination: 'archive', scope: 'group' },
      noGrouping,
    );
    expect(ids).toEqual(['disk:/w/notes.md']);
  });
});

describe('planRelocation', () => {
  it('moves one item and leaves the rest of the plan alone', () => {
    const conflicts = [conflictFor('/a/notes.md', 'notes.md')];
    const context = contextOf([
      { source: '/a/notes.md', destination: 'notes.md', incoming: 'file' },
      { source: '/a/other.md', destination: 'other.md', incoming: 'file' },
    ]);

    const plan = planRelocation(
      conflicts,
      { conflictId: 'disk:/a/notes.md', destination: 'archive', scope: 'one' },
      context,
    );

    expect(plan.moved).toEqual(['disk:/a/notes.md']);
    expect(plan.items).toEqual([
      { source: '/a/notes.md', destination: 'archive/notes.md', incoming: 'file' },
      { source: '/a/other.md', destination: 'other.md', incoming: 'file' },
    ]);
  });

  it('keeps child paths relative to the group root when a whole group moves', () => {
    // The example from the requirement, exactly: an unwrapped group whose
    // children land at the project root.
    const items: PlannedItem[] = [
      { source: '/w/Dashboard/package.json', destination: 'package.json', incoming: 'file' },
      { source: '/w/Dashboard/src/app.ts', destination: 'src/app.ts', incoming: 'file' },
    ];
    const grouping: PlanGrouping = {
      groupOf: { '/w/Dashboard/package.json': 'dash', '/w/Dashboard/src/app.ts': 'dash' },
      rootOf: { dash: '' },
    };
    const conflicts = [conflictFor('/w/Dashboard/package.json', 'package.json')];

    const plan = planRelocation(
      conflicts,
      {
        conflictId: 'disk:/w/Dashboard/package.json',
        destination: 'Projects/NewDashboard',
        scope: 'group',
      },
      contextOf(items, [], grouping),
    );

    expect(plan.items.map((item) => item.destination)).toEqual([
      'Projects/NewDashboard/package.json',
      'Projects/NewDashboard/src/app.ts',
    ]);
  });

  it('keeps a wrapper group under its new parent without flattening it', () => {
    const items: PlannedItem[] = [
      { source: '/w/Dashboard', destination: 'Dashboard', incoming: 'directory' },
      { source: '/w/Dashboard/src/app.ts', destination: 'Dashboard/src/app.ts', incoming: 'file' },
    ];
    const grouping: PlanGrouping = {
      groupOf: { '/w/Dashboard': 'dash', '/w/Dashboard/src/app.ts': 'dash' },
      rootOf: { dash: 'Dashboard' },
    };
    const conflicts = [conflictFor('/w/Dashboard', 'Dashboard', 'directory', 'directory')];

    const plan = planRelocation(
      conflicts,
      { conflictId: 'disk:/w/Dashboard', destination: 'Projects/NewDashboard', scope: 'group' },
      contextOf(items, [], grouping),
    );

    expect(plan.items.map((item) => item.destination)).toEqual([
      'Projects/NewDashboard',
      'Projects/NewDashboard/src/app.ts',
    ]);
  });

  it('does not drag another group along when a group root is the project root', () => {
    const items: PlannedItem[] = [
      { source: '/w/Dashboard/package.json', destination: 'package.json', incoming: 'file' },
      { source: '/w/notes.md', destination: 'notes.md', incoming: 'file' },
    ];
    const grouping: PlanGrouping = {
      groupOf: { '/w/Dashboard/package.json': 'dash', '/w/notes.md': 'loose' },
      rootOf: { dash: '', loose: '' },
    };
    const conflicts = [conflictFor('/w/Dashboard/package.json', 'package.json')];

    const plan = planRelocation(
      conflicts,
      { conflictId: 'disk:/w/Dashboard/package.json', destination: 'Projects', scope: 'group' },
      contextOf(items, [], grouping),
    );

    expect(plan.items.map((item) => item.destination)).toEqual([
      'Projects/package.json',
      'notes.md',
    ]);
  });

  it('moves the compatible conflicts and reports the ones it refused', () => {
    const items: PlannedItem[] = [
      { source: '/a/notes.md', destination: 'notes.md', incoming: 'file' },
      { source: '/a/archive', destination: 'archive', incoming: 'directory' },
    ];
    const conflicts = [
      conflictFor('/a/notes.md', 'notes.md'),
      conflictFor('/a/archive', 'archive', 'directory', 'directory'),
    ];

    const plan = planRelocation(
      conflicts,
      { conflictId: 'disk:/a/notes.md', destination: 'archive', scope: 'all' },
      contextOf(items),
    );

    // `notes.md` can go into `archive`; `archive` itself cannot.
    expect(plan.moved).toEqual(['disk:/a/notes.md']);
    expect(plan.refused).toHaveLength(1);
    expect(plan.refused[0]?.message).toMatch(/into itself/);
  });

  it('previews the final path of everything it moved', () => {
    const items: PlannedItem[] = [
      { source: '/a/notes.md', destination: 'notes.md', incoming: 'file' },
    ];
    const plan = planRelocation(
      [conflictFor('/a/notes.md', 'notes.md')],
      { conflictId: 'disk:/a/notes.md', destination: 'archive', scope: 'one' },
      contextOf(items),
    );
    expect(plan.preview['/a/notes.md']).toBe('archive/notes.md');
  });
});

describe('previewFinalPath', () => {
  it('shows the name under the new folder for a single item', () => {
    const conflict = conflictFor('/a/notes.md', 'docs/notes.md');
    expect(previewFinalPath(conflict, 'archive', 'one', noGrouping)).toBe('archive/notes.md');
  });

  it('shows the rebased path for a group member', () => {
    const grouping: PlanGrouping = {
      groupOf: { '/w/Dashboard/src/app.ts': 'dash' },
      rootOf: { dash: 'Dashboard' },
    };
    const conflict = conflictFor('/w/Dashboard/src/app.ts', 'Dashboard/src/app.ts');
    expect(previewFinalPath(conflict, 'Projects/New', 'group', grouping)).toBe(
      'Projects/New/src/app.ts',
    );
  });
});

describe('preserveDecisions', () => {
  const before = [conflictFor('/a/one.md', 'one.md'), conflictFor('/a/two.md', 'two.md')];
  const decisions: Decisions = {
    'disk:/a/one.md': { resolution: 'replace' },
    'disk:/a/two.md': { resolution: 'skip' },
  };

  it('keeps a decision when its conflict did not move', () => {
    const kept = preserveDecisions(decisions, before, before);
    expect(kept).toEqual(decisions);
  });

  it('drops a decision when its destination changed', () => {
    const after = [conflictFor('/a/one.md', 'archive/one.md'), before[1]!];
    const kept = preserveDecisions(decisions, before, after);
    expect(kept['disk:/a/one.md']).toBeUndefined();
    expect(kept['disk:/a/two.md']).toEqual({ resolution: 'skip' });
  });

  it('drops a decision the caller says was invalidated', () => {
    const kept = preserveDecisions(decisions, before, before, ['disk:/a/two.md']);
    expect(kept['disk:/a/one.md']).toEqual({ resolution: 'replace' });
    expect(kept['disk:/a/two.md']).toBeUndefined();
  });

  it('leaves a brand new conflict unresolved', () => {
    const after = [...before, conflictFor('/a/three.md', 'three.md')];
    const kept = preserveDecisions(decisions, before, after);
    expect(kept['disk:/a/three.md']).toBeUndefined();
  });

  it('forgets a conflict that no longer exists', () => {
    const kept = preserveDecisions(decisions, before, [before[0]!]);
    expect(Object.keys(kept)).toEqual(['disk:/a/one.md']);
  });
});

describe('relocation feeds straight back into analysis', () => {
  it('turns a resolved batch into a new one with the new collisions in it', () => {
    const items: PlannedItem[] = [
      { source: '/a/notes.md', destination: 'notes.md', incoming: 'file' },
    ];
    const existing = new Map<string, ItemKind>([
      ['notes.md', 'file'],
      ['archive/notes.md', 'file'],
    ]);

    const first = allConflicts(analyse(items, existing, 'import'));
    expect(first).toHaveLength(1);

    const plan = planRelocation(
      first,
      { conflictId: 'disk:/a/notes.md', destination: 'archive', scope: 'one' },
      { items, existing, grouping: noGrouping },
    );

    // Moving it did not resolve anything: there is a `notes.md` there too, and
    // the user has to be shown the new collision rather than it being written.
    const second = allConflicts(analyse(plan.items, existing, 'import'));
    expect(second).toHaveLength(1);
    expect(second[0]?.destination).toBe('archive/notes.md');

    // And the decision made about the old collision does not carry over.
    const kept = preserveDecisions(
      { 'disk:/a/notes.md': { resolution: 'replace' } },
      first,
      second,
    );
    expect(kept).toEqual({});
  });
});

describe('pickableDirectories', () => {
  it('lists project directories once, sorted, without the root', () => {
    const list = pickableDirectories(['src', 'docs', 'src'], ['archive', '']);
    expect(list).toEqual(['archive', 'docs', 'src']);
  });
});

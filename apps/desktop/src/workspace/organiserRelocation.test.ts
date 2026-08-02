/**
 * The translation between "this landing path" and "this group".
 *
 * These are the tests that matter for the requirement that a destination
 * chosen in the conflict dialog reaches the *backend* plan: the organiser's
 * groups are what the core re-plans from, so a relocation the groups do not
 * record is a relocation that does not happen.
 */
import { describe, expect, it } from 'vitest';

import type { ImportCandidate, PlannedDestination } from '../api';
import { allConflicts, analyse, type ItemKind, type PlannedItem } from './conflictResolution';
import { groupsFrom, planFrom, setDestination, setLayout } from './importGroups';
import { groupingFor, relocateGroups } from './organiserRelocation';
import { planRelocation, type RelocationRequest } from './relocation';

function candidate(overrides: Partial<ImportCandidate> & { path: string }): ImportCandidate {
  return {
    name: overrides.path.split('/').pop() ?? overrides.path,
    isDirectory: true,
    isProject: false,
    score: 0,
    signals: [],
    children: [],
    childCount: 0,
    ecosystem: null,
    isMonorepo: false,
    nested: [],
    ...overrides,
  };
}

const dashboard = candidate({
  path: '/w/Dashboard',
  name: 'Dashboard',
  isProject: true,
  score: 80,
  signals: ['package.json'],
  ecosystem: 'node',
});
const assets = candidate({ path: '/w/Assets', name: 'Assets' });
const notes = candidate({ path: '/w/notes.md', name: 'notes.md', isDirectory: false });

function planned(entries: [string, string, boolean?][]): PlannedDestination[] {
  return entries.map(([source, relative, isDirectory = false]) => ({
    source,
    relative,
    isDirectory,
    totalFiles: 1,
    totalBytes: 10,
    existing: null,
  }));
}

function conflictsFor(items: PlannedItem[], disk: Record<string, ItemKind>) {
  return allConflicts(analyse(items, new Map(Object.entries(disk)), 'import'));
}

describe('groupingFor', () => {
  it('links a kept group to its own landing', () => {
    const groups = groupsFrom([assets], '');
    const grouping = groupingFor(planned([['/w/Assets', 'Assets', true]]), groups);
    expect(grouping.groupOf['/w/Assets']).toBe('/w/Assets');
    expect(grouping.rootOf['/w/Assets']).toBe('');
  });

  it('links an unwrapped group to each of its children', () => {
    const groups = groupsFrom([dashboard], '');
    const grouping = groupingFor(
      planned([
        ['/w/Dashboard/package.json', 'package.json'],
        ['/w/Dashboard/src', 'src', true],
      ]),
      groups,
    );
    expect(grouping.groupOf['/w/Dashboard/package.json']).toBe('/w/Dashboard');
    expect(grouping.groupOf['/w/Dashboard/src']).toBe('/w/Dashboard');
  });

  it('records a custom destination as the group root', () => {
    const groups = setDestination(groupsFrom([assets], ''), '/w/Assets', 'media');
    const grouping = groupingFor(planned([['/w/Assets', 'media/Assets', true]]), groups);
    expect(grouping.rootOf['/w/Assets']).toBe('media');
  });
});

describe('relocating a whole group', () => {
  it('changes the group destination, which is what the core re-plans from', () => {
    const groups = groupsFrom([dashboard, notes], '');
    const destinations = planned([
      ['/w/Dashboard/package.json', 'package.json'],
      ['/w/Dashboard/src', 'src', true],
      ['/w/notes.md', 'notes.md'],
    ]);
    const conflicts = conflictsFor(
      [
        { source: '/w/Dashboard/package.json', destination: 'package.json', incoming: 'file' },
        { source: '/w/notes.md', destination: 'notes.md', incoming: 'file' },
      ],
      { 'package.json': 'file', 'notes.md': 'file' },
    );

    const request: RelocationRequest = {
      conflictId: 'disk:/w/Dashboard/package.json',
      destination: 'Projects/NewDashboard',
      scope: 'group',
    };
    const result = relocateGroups(groups, destinations, conflicts, request, [
      'disk:/w/Dashboard/package.json',
    ]);

    const moved = result.groups.find((group) => group.id === '/w/Dashboard');
    expect(moved?.destination).toBe('Projects/NewDashboard');
    // The other group is untouched.
    expect(result.groups.find((group) => group.id === 'standalone')?.destination).toBe('');
  });

  it('keeps child paths relative to the group root once the core re-plans', () => {
    // The requirement's example: the plan the core would be asked for after the
    // destination change lands the children under the new root, not flattened.
    const groups = setDestination(
      groupsFrom([dashboard], ''),
      '/w/Dashboard',
      'Projects/NewDashboard',
    );
    const plan = planFrom(groups);
    expect(plan.batches).toEqual([
      {
        destination: 'Projects/NewDashboard',
        sourcePaths: ['/w/Dashboard'],
        unwrapPaths: ['/w/Dashboard'],
      },
    ]);
    // The core is asked to unwrap `/w/Dashboard` into `Projects/NewDashboard`,
    // so `src/app.ts` lands at `Projects/NewDashboard/src/app.ts` by
    // construction — the window never rebuilds that path itself.
  });

  it('moves a wrapper group under its new parent', () => {
    const groups = groupsFrom([assets], '');
    const destinations = planned([['/w/Assets', 'Assets', true]]);
    const conflicts = conflictsFor(
      [{ source: '/w/Assets', destination: 'Assets', incoming: 'directory' }],
      { Assets: 'directory' },
    );

    const result = relocateGroups(
      groups,
      destinations,
      conflicts,
      { conflictId: 'disk:/w/Assets', destination: 'media', scope: 'group' },
      ['disk:/w/Assets'],
    );

    expect(planFrom(result.groups).destinations.map((entry) => entry.path)).toEqual([
      'media/Assets',
    ]);
  });

  it('keeps a rename when the group moves', () => {
    let groups = groupsFrom([assets], '');
    groups = groups.map((group) =>
      group.id === '/w/Assets' ? { ...group, name: 'Artwork' } : group,
    );
    const destinations = planned([['/w/Assets', 'Artwork', true]]);
    const conflicts = conflictsFor(
      [{ source: '/w/Assets', destination: 'Artwork', incoming: 'directory' }],
      { Artwork: 'directory' },
    );

    const result = relocateGroups(
      groups,
      destinations,
      conflicts,
      { conflictId: 'disk:/w/Assets', destination: 'media', scope: 'group' },
      ['disk:/w/Assets'],
    );

    expect(planFrom(result.groups).destinations.map((entry) => entry.path)).toEqual([
      'media/Artwork',
    ]);
  });

  it('carries every entry that inherits the group destination', () => {
    const groups = groupsFrom([notes, candidate({ path: '/w/todo.md', isDirectory: false })], '');
    const destinations = planned([
      ['/w/notes.md', 'notes.md'],
      ['/w/todo.md', 'todo.md'],
    ]);
    const conflicts = conflictsFor(
      [
        { source: '/w/notes.md', destination: 'notes.md', incoming: 'file' },
        { source: '/w/todo.md', destination: 'todo.md', incoming: 'file' },
      ],
      { 'notes.md': 'file', 'todo.md': 'file' },
    );

    const result = relocateGroups(
      groups,
      destinations,
      conflicts,
      { conflictId: 'disk:/w/notes.md', destination: 'archive', scope: 'group' },
      ['disk:/w/notes.md', 'disk:/w/todo.md'],
    );

    expect(planFrom(result.groups).destinations.map((entry) => entry.path)).toEqual([
      'archive/notes.md',
      'archive/todo.md',
    ]);
  });
});

describe('relocating one entry out of a group', () => {
  it('splits a standalone file into its own group with the new destination', () => {
    const groups = groupsFrom([notes, candidate({ path: '/w/todo.md', isDirectory: false })], '');
    const destinations = planned([
      ['/w/notes.md', 'notes.md'],
      ['/w/todo.md', 'todo.md'],
    ]);
    const conflicts = conflictsFor(
      [
        { source: '/w/notes.md', destination: 'notes.md', incoming: 'file' },
        { source: '/w/todo.md', destination: 'todo.md', incoming: 'file' },
      ],
      { 'notes.md': 'file', 'todo.md': 'file' },
    );

    const result = relocateGroups(
      groups,
      destinations,
      conflicts,
      { conflictId: 'disk:/w/notes.md', destination: 'archive', scope: 'one' },
      ['disk:/w/notes.md'],
    );

    const paths = planFrom(result.groups).destinations.map((entry) => entry.path);
    expect(paths).toContain('archive/notes.md');
    expect(paths).toContain('todo.md');
  });

  it('refuses to move one child of an unwrapped group on its own', () => {
    // The group unwraps into a single directory by definition, so its contents
    // cannot land in two places. Saying so beats moving the whole group behind
    // the user's back.
    const groups = groupsFrom([dashboard], '');
    const destinations = planned([
      ['/w/Dashboard/package.json', 'package.json'],
      ['/w/Dashboard/src', 'src', true],
    ]);
    const conflicts = conflictsFor(
      [
        { source: '/w/Dashboard/package.json', destination: 'package.json', incoming: 'file' },
        { source: '/w/Dashboard/src', destination: 'src', incoming: 'directory' },
      ],
      { 'package.json': 'file', src: 'directory' },
    );

    const result = relocateGroups(
      groups,
      destinations,
      conflicts,
      { conflictId: 'disk:/w/Dashboard/package.json', destination: 'archive', scope: 'one' },
      ['disk:/w/Dashboard/package.json'],
    );

    expect(result.refused).toHaveLength(1);
    expect(result.refused[0]?.message).toMatch(/move together/);
    // And nothing moved.
    expect(result.groups.find((group) => group.id === '/w/Dashboard')?.destination).toBe('');
  });

  it('leaves excluded groups alone', () => {
    let groups = groupsFrom([assets, notes], '');
    groups = groups.map((group) =>
      group.id === '/w/Assets' ? { ...group, include: false } : group,
    );
    const destinations = planned([['/w/notes.md', 'notes.md']]);
    const conflicts = conflictsFor(
      [{ source: '/w/notes.md', destination: 'notes.md', incoming: 'file' }],
      { 'notes.md': 'file' },
    );

    const result = relocateGroups(
      groups,
      destinations,
      conflicts,
      { conflictId: 'disk:/w/notes.md', destination: 'archive', scope: 'all' },
      ['disk:/w/notes.md'],
    );

    const excluded = result.groups.find((group) => group.id === '/w/Assets');
    expect(excluded?.include).toBe(false);
    expect(excluded?.destination).toBe('');
  });
});

describe('the whole round trip', () => {
  it('turns a chosen destination into a batch the core is actually asked for', () => {
    const groups = setLayout(groupsFrom([dashboard], ''), '/w/Dashboard', 'unwrap');
    const destinations = planned([
      ['/w/Dashboard/package.json', 'package.json'],
      ['/w/Dashboard/src', 'src', true],
    ]);
    const items: PlannedItem[] = [
      { source: '/w/Dashboard/package.json', destination: 'package.json', incoming: 'file' },
      { source: '/w/Dashboard/src', destination: 'src', incoming: 'directory' },
    ];
    const existing = new Map<string, ItemKind>([['package.json', 'file']]);
    const conflicts = allConflicts(analyse(items, existing, 'import'));

    const grouping = groupingFor(destinations, groups);
    const request: RelocationRequest = {
      conflictId: conflicts[0]!.id,
      destination: 'Projects/NewDashboard',
      scope: 'group',
    };
    const targets = planRelocation(conflicts, request, { items, existing, grouping });
    const moved = relocateGroups(groups, destinations, conflicts, request, targets.moved);

    // What the core will be told to do, which is the only thing that matters.
    expect(planFrom(moved.groups).batches).toEqual([
      {
        destination: 'Projects/NewDashboard',
        sourcePaths: ['/w/Dashboard'],
        unwrapPaths: ['/w/Dashboard'],
      },
    ]);
    expect(moved.summary).toMatch(/Projects\/NewDashboard/);
  });
});

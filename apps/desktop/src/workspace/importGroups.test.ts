import { describe, expect, it } from 'vitest';

import type { ImportCandidate, NestedProject } from '../api';
import {
  conflictsWith,
  createGroup,
  groupsFrom,
  mergeGroups,
  moveEntries,
  planFrom,
  removeEntries,
  renameGroup,
  resetGroup,
  setDestination,
  setInclude,
  setKind,
  setLayout,
  STANDALONE_ID,
  summarise,
} from './importGroups';

function candidate(patch: Partial<ImportCandidate> & { name: string }): ImportCandidate {
  return {
    path: `C:\\drop\\${patch.name}`,
    isDirectory: true,
    isProject: false,
    score: 0,
    signals: [],
    children: [],
    childCount: 0,
    ecosystem: null,
    isMonorepo: false,
    nested: [],
    ...patch,
  };
}

function projectCandidate(name: string, extra: Partial<ImportCandidate> = {}): ImportCandidate {
  return candidate({
    name,
    isProject: true,
    score: 5,
    signals: ['package.json', 'src/'],
    ecosystem: 'Node.js',
    ...extra,
  });
}

function nested(name: string, belongsToWorkspace: boolean): NestedProject {
  return {
    path: `C:\\drop\\mono\\packages\\${name}`,
    relative: `packages/${name}`,
    name,
    ecosystem: 'Node.js',
    score: 5,
    belongsToWorkspace,
  };
}

describe('building the starting plan', () => {
  it('makes one group per detected project', () => {
    const groups = groupsFrom([projectCandidate('bot'), projectCandidate('dashboard')], '');
    expect(groups.filter((group) => group.kind === 'project')).toHaveLength(2);
  });

  it('collects loose files into one clearly separate group', () => {
    // Mixing them into a project is how notes.txt disappears into a source tree.
    const groups = groupsFrom(
      [projectCandidate('bot'), candidate({ name: 'notes.txt', isDirectory: false })],
      '',
    );
    const standalone = groups.find((group) => group.id === STANDALONE_ID);
    expect(standalone?.entries.map((entry) => entry.name)).toEqual(['notes.txt']);
  });

  it('keeps ordinary folders as their own groups', () => {
    const groups = groupsFrom([candidate({ name: 'assets' }), candidate({ name: 'docs' })], '');
    expect(groups.map((group) => group.name)).toEqual(['assets', 'docs']);
    expect(groups.every((group) => group.kind === 'folder')).toBe(true);
  });

  it('unwraps a single project so its contents land at the destination', () => {
    const [group] = groupsFrom([projectCandidate('RomiPlayoff')], '');
    expect(group?.layout).toBe('unwrap');
  });

  it('keeps both folders when two projects want the same destination', () => {
    // Interleaving two projects into one root cannot be undone afterwards.
    const groups = groupsFrom([projectCandidate('bot'), projectCandidate('dashboard')], '');
    expect(groups.every((group) => group.layout === 'keep')).toBe(true);
  });

  it('explains what it decided and why', () => {
    const [group] = groupsFrom([projectCandidate('bot')], '');
    expect(group?.detection?.explanation).toContain('Node.js project');
    expect(group?.detection?.explanation).toContain('package.json');
    expect(group?.detection?.explanation).toContain('score 5');
  });

  it('explains a folder it decided against', () => {
    const weak = candidate({ name: 'thing', signals: ['.gitignore'], score: 1 });
    const [group] = groupsFrom([weak], '');
    expect(group?.detection?.explanation).toContain('not enough');
  });

  it('lists a monorepo’s members but keeps them inside it', () => {
    const mono = projectCandidate('mono', {
      isMonorepo: true,
      nested: [nested('api', true), nested('web', true)],
    });
    const groups = groupsFrom([mono], '');
    expect(groups).toHaveLength(1);
    expect(groups[0]?.detection?.members).toEqual(['packages/api', 'packages/web']);
  });

  it('does not treat an independent nested project as a workspace member', () => {
    const outer = projectCandidate('outer', { nested: [nested('inner', false)] });
    const [group] = groupsFrom([outer], '');
    expect(group?.detection?.members).toEqual([]);
  });

  it('honours the destination it was given', () => {
    const [group] = groupsFrom([projectCandidate('bot')], 'vendor');
    expect(group?.destination).toBe('vendor');
  });
});

describe('editing the plan', () => {
  const base = groupsFrom(
    [
      projectCandidate('bot'),
      candidate({ name: 'assets' }),
      candidate({ name: 'a.txt', isDirectory: false }),
    ],
    '',
  );
  const botId = 'C:\\drop\\bot';
  const assetsId = 'C:\\drop\\assets';

  it('excludes a group without removing it from view', () => {
    const groups = setInclude(base, botId, false);
    expect(groups.find((group) => group.id === botId)?.include).toBe(false);
    expect(groups).toHaveLength(base.length);
  });

  it('renames a group', () => {
    expect(renameGroup(base, botId, 'Discord Bot').find((g) => g.id === botId)?.name).toBe(
      'Discord Bot',
    );
  });

  it('changes a folder into a project and unwraps it', () => {
    // On its own, with nothing else claiming the destination.
    const alone = groupsFrom([candidate({ name: 'assets' })], '');
    const groups = setKind(alone, assetsId, 'project');
    const assets = groups.find((group) => group.id === assetsId);
    expect(assets?.kind).toBe('project');
    expect(assets?.layout).toBe('unwrap');
  });

  it('changes a project into a folder and keeps its wrapper', () => {
    const groups = setKind(base, botId, 'folder');
    expect(groups.find((group) => group.id === botId)?.layout).toBe('keep');
  });

  it('marks a group as no longer automatic once it is touched', () => {
    expect(base.find((group) => group.id === botId)?.auto).toBe(true);
    expect(renameGroup(base, botId, 'x').find((group) => group.id === botId)?.auto).toBe(false);
  });

  it('resets a group to what detection recommended', () => {
    const candidates = [projectCandidate('bot')];
    const edited = setKind(renameGroup(groupsFrom(candidates, ''), botId, 'x'), botId, 'folder');
    const reset = resetGroup(edited, botId, candidates, '');
    expect(reset.find((group) => group.id === botId)?.name).toBe('bot');
    expect(reset.find((group) => group.id === botId)?.kind).toBe('project');
  });

  it('changes a destination', () => {
    expect(setDestination(base, botId, 'vendor').find((g) => g.id === botId)?.destination).toBe(
      'vendor',
    );
  });

  /** Two groups unwrapping into one place would interleave their files. */
  function unwrapsPerDestination(groups: ReturnType<typeof groupsFrom>) {
    const counts = new Map<string, number>();
    for (const group of groups) {
      if (group.kind === 'files' || group.layout !== 'unwrap' || !group.include) continue;
      counts.set(group.destination, (counts.get(group.destination) ?? 0) + 1);
    }
    return counts;
  }

  it('never lets two groups unwrap into the same destination', () => {
    const groups = setLayout(setKind(base, assetsId, 'project'), assetsId, 'unwrap');
    for (const count of unwrapsPerDestination(groups).values()) {
      expect(count).toBeLessThanOrEqual(1);
    }
  });

  it('allows two unwraps when their destinations differ', () => {
    // Each project asked for its own folder, so neither can tread on the other.
    const separate = setDestination(
      groupsFrom([projectCandidate('bot'), projectCandidate('dash')], ''),
      'C:\\drop\\dash',
      'vendor',
    );
    const restored = setLayout(
      setLayout(separate, 'C:\\drop\\bot', 'unwrap'),
      'C:\\drop\\dash',
      'unwrap',
    );
    const unwrapping = restored.filter(
      (group) => group.layout === 'unwrap' && group.kind !== 'files',
    );
    expect(unwrapping).toHaveLength(2);
  });
});

describe('moving entries between groups', () => {
  const groups = groupsFrom(
    [
      projectCandidate('bot'),
      candidate({ name: 'a.txt', isDirectory: false }),
      candidate({ name: 'b.txt', isDirectory: false }),
    ],
    '',
  );

  it('moves an entry into another group', () => {
    const moved = moveEntries(groups, STANDALONE_ID, 'C:\\drop\\bot', ['C:\\drop\\a.txt']);
    expect(moved.find((g) => g.id === 'C:\\drop\\bot')?.entries).toHaveLength(2);
    expect(moved.find((g) => g.id === STANDALONE_ID)?.entries).toHaveLength(1);
  });

  it('drops a group that has been emptied', () => {
    const moved = moveEntries(groups, STANDALONE_ID, 'C:\\drop\\bot', [
      'C:\\drop\\a.txt',
      'C:\\drop\\b.txt',
    ]);
    expect(moved.find((group) => group.id === STANDALONE_ID)).toBeUndefined();
  });

  it('does nothing when the source and destination are the same', () => {
    expect(moveEntries(groups, STANDALONE_ID, STANDALONE_ID, ['C:\\drop\\a.txt'])).toBe(groups);
  });

  it('does not duplicate an entry already in the destination', () => {
    const once = moveEntries(groups, STANDALONE_ID, 'C:\\drop\\bot', ['C:\\drop\\a.txt']);
    const twice = moveEntries(once, STANDALONE_ID, 'C:\\drop\\bot', ['C:\\drop\\a.txt']);
    expect(twice.find((g) => g.id === 'C:\\drop\\bot')?.entries).toHaveLength(2);
  });

  it('removes entries from the import entirely', () => {
    const left = removeEntries(groups, ['C:\\drop\\a.txt']);
    expect(left.find((g) => g.id === STANDALONE_ID)?.entries.map((e) => e.name)).toEqual(['b.txt']);
  });
});

describe('creating and merging groups', () => {
  const groups = groupsFrom([projectCandidate('bot'), projectCandidate('dash')], '');

  it('adds an empty custom group to drag entries into', () => {
    const next = createGroup(groups, 'Extras', 'folder');
    expect(next).toHaveLength(3);
    expect(next[2]?.entries).toEqual([]);
  });

  it('merges groups only when asked, and keeps the wrapper afterwards', () => {
    const merged = mergeGroups(groups, ['C:\\drop\\bot', 'C:\\drop\\dash']);
    expect(merged).toHaveLength(1);
    expect(merged[0]?.entries).toHaveLength(2);
    // A group holding two things cannot unwrap one of them over the root.
    expect(merged[0]?.layout).toBe('keep');
  });

  it('refuses to merge fewer than two groups', () => {
    expect(mergeGroups(groups, ['C:\\drop\\bot'])).toBe(groups);
  });
});

describe('turning groups into calls', () => {
  it('groups the calls by destination', () => {
    const groups = setDestination(
      groupsFrom([projectCandidate('bot'), candidate({ name: 'assets' })], ''),
      'C:\\drop\\assets',
      'vendor',
    );
    const plan = planFrom(groups);
    expect(plan.batches.map((batch) => batch.destination).sort()).toEqual(['', 'vendor']);
  });

  it('marks the unwrapped folder in its batch', () => {
    const plan = planFrom(groupsFrom([projectCandidate('bot')], ''));
    expect(plan.batches[0]?.unwrapPaths).toEqual(['C:\\drop\\bot']);
  });

  it('leaves a kept folder out of the unwrap list and records where it lands', () => {
    const plan = planFrom(groupsFrom([candidate({ name: 'assets' })], ''));
    expect(plan.batches[0]?.unwrapPaths).toEqual([]);
    expect(plan.destinations).toEqual([{ path: 'assets', from: 'C:\\drop\\assets' }]);
  });

  it('lands a renamed folder under its new name', () => {
    const groups = renameGroup(
      groupsFrom([candidate({ name: 'assets' })], ''),
      'C:\\drop\\assets',
      'Media',
    );
    expect(planFrom(groups).destinations[0]?.path).toBe('Media');
  });

  it('omits excluded groups and counts what was left out', () => {
    const groups = setInclude(groupsFrom([projectCandidate('bot')], ''), 'C:\\drop\\bot', false);
    const plan = planFrom(groups);
    expect(plan.batches).toEqual([]);
    expect(plan.excluded).toBe(1);
  });

  it('places standalone files at the group destination', () => {
    const groups = groupsFrom([candidate({ name: 'a.txt', isDirectory: false })], 'docs');
    expect(planFrom(groups).destinations).toEqual([
      { path: 'docs/a.txt', from: 'C:\\drop\\a.txt' },
    ]);
  });
});

describe('conflicts', () => {
  it('reports destinations that already exist', () => {
    const plan = planFrom(groupsFrom([candidate({ name: 'assets' })], ''));
    expect(conflictsWith(plan, ['assets', 'other'])).toEqual(['assets']);
  });

  it('matches case-insensitively, as the filesystem does', () => {
    const plan = planFrom(groupsFrom([candidate({ name: 'Assets' })], ''));
    expect(conflictsWith(plan, ['assets'])).toEqual(['Assets']);
  });

  it('reports nothing when the destination is clear', () => {
    const plan = planFrom(groupsFrom([candidate({ name: 'assets' })], ''));
    expect(conflictsWith(plan, ['src'])).toEqual([]);
  });
});

describe('summarising', () => {
  it('counts each kind', () => {
    const groups = groupsFrom(
      [
        projectCandidate('bot'),
        candidate({ name: 'assets' }),
        candidate({ name: 'a.txt', isDirectory: false }),
        candidate({ name: 'b.txt', isDirectory: false }),
      ],
      '',
    );
    expect(summarise(groups)).toBe('Importing 1 project, 1 folder and 2 files.');
  });

  it('says plainly when everything has been excluded', () => {
    const groups = setInclude(groupsFrom([projectCandidate('bot')], ''), 'C:\\drop\\bot', false);
    expect(summarise(groups)).toBe('Nothing selected to import.');
  });
});

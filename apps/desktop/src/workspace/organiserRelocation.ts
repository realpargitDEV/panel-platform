/**
 * Turning a destination change into an edit of the approved import plan.
 *
 * The conflict dialog talks about *landing paths*; the organiser talks about
 * *groups*. This is the translation between them, and it has to be exact: the
 * backend re-plans from the groups, so a relocation the groups do not record is
 * a relocation that silently does not happen.
 *
 * The rule that makes it work is that a group's landing paths are all rebased
 * from one root — the group's own destination — whether it keeps its folder or
 * unwraps it. Moving the group is therefore a single change to
 * `group.destination`, and every child path follows from re-planning rather
 * than from string surgery here. That is what keeps `src/app.ts` at
 * `src/app.ts` instead of being rebuilt from its file name.
 */
import type { PlannedDestination } from '../api';
import type { Conflict } from './conflictResolution';
import { samePath } from './conflictResolution';
import { createGroup, moveEntries, setDestination, type ImportGroup } from './importGroups';
import { normaliseDirectory, type PlanGrouping, type RelocationRequest } from './relocation';

export interface OrganiserRelocation {
  groups: ImportGroup[];
  /** What changed, for the output panel and for the dialog's notice. */
  summary: string;
  /** Conflicts the plan could not express, with why. */
  refused: { id: string; message: string }[];
}

/**
 * Which group each planned destination came from.
 *
 * A planned destination is a *top-level* landing: the group's own folder when
 * it is kept, or one of its children when it is unwrapped. Both are matched by
 * absolute source path against the group's entries, which is the only link the
 * two sides share — the relative paths are what is being changed.
 */
export function groupingFor(planned: PlannedDestination[], groups: ImportGroup[]): PlanGrouping {
  const groupOf: Record<string, string> = {};
  const rootOf: Record<string, string> = {};

  for (const group of groups) {
    rootOf[group.id] = group.destination;
  }

  for (const entry of planned) {
    const owner = groups.find((group) =>
      group.entries.some(
        (candidate) =>
          samePath(candidate.path, entry.source) || isUnder(entry.source, candidate.path),
      ),
    );
    if (owner) groupOf[entry.source] = owner.id;
  }

  return { groupOf, rootOf };
}

/** Absolute-path containment, tolerant of either separator. */
function isUnder(path: string, root: string): boolean {
  const left = path.replace(/\\/g, '/').toLowerCase();
  const right = root.replace(/\\/g, '/').replace(/\/+$/, '').toLowerCase();
  return left.startsWith(`${right}/`);
}

/**
 * Apply a relocation to the groups the import will actually run from.
 *
 * Whole-group moves are one edit to the group's destination. Anything narrower
 * splits the affected *entries* into a group of their own, which is how a
 * standalone file ends up somewhere different from the rest of its batch
 * without disturbing the others.
 *
 * A conflict on a child of an unwrapped group cannot be moved on its own — the
 * group unwraps into one directory by definition — so it is refused with that
 * reason rather than quietly moving the whole group.
 */
export function relocateGroups(
  groups: ImportGroup[],
  planned: PlannedDestination[],
  conflicts: Conflict[],
  request: RelocationRequest,
  targetIds: string[],
): OrganiserRelocation {
  const destination = normaliseDirectory(request.destination);
  const grouping = groupingFor(planned, groups);
  const byId = new Map(conflicts.map((conflict) => [conflict.id, conflict]));
  const refused: { id: string; message: string }[] = [];

  /** Group id to the sources within it that are moving. */
  const movingByGroup = new Map<string, Set<string>>();
  for (const id of targetIds) {
    const conflict = byId.get(id);
    if (!conflict) continue;
    const groupId = grouping.groupOf[conflict.source];
    if (groupId === undefined) {
      refused.push({ id, message: 'That item is not part of any group in this import.' });
      continue;
    }
    const sources = movingByGroup.get(groupId) ?? new Set<string>();
    sources.add(conflict.source);
    movingByGroup.set(groupId, sources);
  }

  let next = groups;
  const moved: string[] = [];

  for (const [groupId, sources] of movingByGroup) {
    const group = next.find((candidate) => candidate.id === groupId);
    if (!group) continue;

    const plannedForGroup = planned.filter((entry) => grouping.groupOf[entry.source] === groupId);
    const whole =
      request.scope === 'group' || plannedForGroup.every((entry) => sources.has(entry.source));

    if (whole) {
      next = setDestination(next, groupId, destination);
      moved.push(group.name);
      continue;
    }

    // A narrower move: only the entries the user actually picked. An entry is a
    // top-level thing the group holds, so a source that is only a *child* of one
    // (an unwrapped folder's contents) cannot be split out.
    const entries = group.entries.filter((entry) => sources.has(entry.path));
    const unsplittable = [...sources].filter(
      (source) => !group.entries.some((entry) => samePath(entry.path, source)),
    );
    for (const source of unsplittable) {
      const conflict = conflicts.find((candidate) => candidate.source === source);
      if (conflict) {
        refused.push({
          id: conflict.id,
          message: `${group.name} unwraps into one folder, so its contents move together. Choose "Everything in this group".`,
        });
      }
    }
    if (entries.length === 0) continue;

    const name = `${group.name} (moved)`;
    const withGroup = createGroup(next, name, group.kind);
    const created = withGroup[withGroup.length - 1];
    if (!created) continue;
    next = moveEntries(
      withGroup,
      groupId,
      created.id,
      entries.map((entry) => entry.path),
    );
    next = setDestination(next, created.id, destination);
    moved.push(entries.map((entry) => entry.name).join(', '));
  }

  const where = destination === '' ? 'the project root' : destination;
  const summary =
    moved.length === 0
      ? 'Nothing could be moved to the new destination.'
      : `${moved.join(', ')} will now land in ${where}. The conflicts have been checked again.`;

  return { groups: next, summary, refused };
}

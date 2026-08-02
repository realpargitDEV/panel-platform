/**
 * Sending a conflicted item somewhere else.
 *
 * The fourth answer to "this already exists". Replace, keep both and skip all
 * accept the destination as given; this one changes it. It belongs beside them
 * in the same dialog, because the alternative — cancel, edit the organiser,
 * start again — throws away every other decision the user has already made.
 *
 * Everything here is a pure function over the *plan*. No path is written and no
 * directory is read: a relocation produces a new set of planned destinations,
 * which then go back through the same analysis as the original ones. That is
 * what makes "re-run the conflict check after the destination changes" a
 * consequence of the design rather than a step someone has to remember.
 *
 * The property worth stating plainly: a group is rebased, never rebuilt. Moving
 * `Dashboard/` to `Projects/NewDashboard/` keeps `src/app.ts` at
 * `Projects/NewDashboard/src/app.ts`. Rebuilding the path from the file's name
 * would flatten it to `Projects/NewDashboard/app.ts` and lose the tree — a
 * data-shape bug that still imports successfully and is only found later.
 */
import type { Conflict, Decisions, ItemKind, PlannedItem } from './conflictResolution';
import { normalisePath, samePath } from './conflictResolution';

/** How widely a destination change is applied. */
export type RelocationScope =
  /** Only the conflict being edited. */
  | 'one'
  /** Every conflict whose item came from the same incoming group. */
  | 'group'
  /** Every conflict that was heading for the same directory. */
  | 'same-destination'
  /** Every conflict in the batch that can take the new destination. */
  | 'all';

export interface RelocationRequest {
  /** The conflict the user opened the picker on. */
  conflictId: string;
  /** The project-relative directory chosen. `''` is the project root. */
  destination: string;
  scope: RelocationScope;
}

/**
 * Where each planned item came from, when the batch was organised.
 *
 * Only the organiser has groups; paste, cut and drag leave this empty, and the
 * `group` scope then falls back to the single conflict rather than guessing.
 */
export interface PlanGrouping {
  /** Source path to the id of the group it belongs to. */
  groupOf: Record<string, string>;
  /**
   * Group id to the project-relative directory its members' destinations sit
   * under. `''` for a group landing at the project root — which is exactly what
   * an unwrapped group does, and why this cannot be inferred from the paths.
   */
  rootOf: Record<string, string>;
}

export const noGrouping: PlanGrouping = { groupOf: {}, rootOf: {} };

// --------------------------------------------------------------- path helpers

export function baseNameOf(path: string): string {
  const cut = path.lastIndexOf('/');
  return cut < 0 ? path : path.slice(cut + 1);
}

export function parentOf(path: string): string {
  const cut = path.lastIndexOf('/');
  return cut < 0 ? '' : path.slice(0, cut);
}

/** `a/b` + `c` is `a/b/c`; the project root is `''`, not `/`. */
export function joinRelative(directory: string, name: string): string {
  if (!directory) return name;
  if (!name) return directory;
  return `${directory}/${name}`;
}

/** Is `path` `root` itself, or somewhere inside it? */
export function isAtOrInside(path: string, root: string): boolean {
  if (root === '') return true;
  const left = normalisePath(path);
  const right = normalisePath(root);
  return left === right || left.startsWith(`${right}/`);
}

/** Is `path` strictly inside `root`? */
export function isInside(path: string, root: string): boolean {
  if (samePath(path, root)) return false;
  return isAtOrInside(path, root);
}

/**
 * Move a path from under one root to under another, keeping what is below it.
 *
 * `rebase('Dashboard/src/app.ts', 'Dashboard', 'Projects/NewDashboard')` is
 * `Projects/NewDashboard/src/app.ts`. An empty `oldRoot` means the path is
 * already relative to the group — an unwrapped group's children are — and the
 * whole of it is kept.
 *
 * Returns `null` when the path is not under `oldRoot` at all, so a caller
 * cannot silently rebase something that was never part of the group.
 */
export function rebase(path: string, oldRoot: string, newRoot: string): string | null {
  if (oldRoot === '') return joinRelative(newRoot, path);
  if (!isAtOrInside(path, oldRoot)) return null;
  if (samePath(path, oldRoot)) return newRoot;
  const suffix = path.slice(oldRoot.length + 1);
  return joinRelative(newRoot, suffix);
}

// ----------------------------------------------------------------- validation

export interface DestinationProblem {
  /** Shown to the user, in the dialog, next to the picker. */
  message: string;
}

/** Names Windows will not let a directory have, whatever the extension. */
const RESERVED = new Set([
  'con',
  'prn',
  'aux',
  'nul',
  ...Array.from({ length: 9 }, (_, index) => `com${index + 1}`),
  ...Array.from({ length: 9 }, (_, index) => `lpt${index + 1}`),
]);

/** Characters no platform lets a folder name hold. Spaces and dashes are fine. */
const ILLEGAL = /[<>:"|?*]/;

/** Control characters: every filesystem refuses them and nobody typed one on purpose. */
function hasControlCharacter(segment: string): boolean {
  for (const character of segment) {
    const code = character.codePointAt(0) ?? 0;
    if (code < 32 || code === 127) return true;
  }
  return false;
}

/**
 * Is this a destination the project can actually hold?
 *
 * Refused before anything else looks at it, because every later step — the
 * preview, the re-analysis, the import itself — would otherwise be reasoning
 * about a path that cannot exist. The core would refuse it too; refusing here
 * means the user finds out while the dialog is still open and their other
 * decisions are still on screen.
 */
export function validateDestinationPath(input: string): DestinationProblem | null {
  const path = input.trim();
  if (path.length === 0) {
    return { message: 'Enter a destination folder, or choose the project root.' };
  }
  if (path.startsWith('/') || path.startsWith('\\') || /^[a-z]:/i.test(path)) {
    return { message: 'Use a folder inside the project, not an absolute path.' };
  }

  const segments = path.replace(/\\/g, '/').split('/');
  for (const segment of segments) {
    if (segment === '') return { message: 'That path has an empty folder name in it.' };
    if (segment === '.' || segment === '..') {
      return { message: 'A destination cannot step outside the project with "..".' };
    }
    if (ILLEGAL.test(segment) || hasControlCharacter(segment)) {
      return { message: `"${segment}" contains characters a folder name cannot have.` };
    }
    if (segment !== segment.trimEnd() || segment.endsWith('.')) {
      return { message: `"${segment}" cannot end with a space or a dot.` };
    }
    const stem = segment.split('.')[0] ?? '';
    if (RESERVED.has(stem.toLowerCase())) {
      return { message: `"${segment}" is a reserved name on Windows.` };
    }
  }

  return null;
}

export interface RelocationContext {
  /** Every item the batch plans to land, including the ones with no conflict. */
  items: PlannedItem[];
  /** What is on disk at the destination, and what kind each entry is. */
  existing: Map<string, ItemKind>;
  grouping: PlanGrouping;
}

/** `''` stays `''`; everything else loses its backslashes and trailing separator. */
export function normaliseDirectory(input: string): string {
  return input.trim().replace(/\\/g, '/').replace(/\/+$/, '');
}

function lookupKind(existing: Map<string, ItemKind>, path: string): ItemKind | undefined {
  for (const [key, kind] of existing) {
    if (samePath(key, path)) return kind;
  }
  return undefined;
}

/** The directory a conflict's group sits under, or `null` when it has no group. */
export function rootFor(conflict: Conflict, grouping: PlanGrouping): string | null {
  const groupId = grouping.groupOf[conflict.source];
  if (groupId === undefined) return null;
  return grouping.rootOf[groupId] ?? null;
}

/**
 * Can this one conflict take this destination?
 *
 * Separate from `validateDestinationPath` because these answers depend on what
 * else is in the batch: the same folder is a perfectly good destination for one
 * item and a loop for another. The scope matters too — a `group` relocation
 * moves the group's root, so it is the *root* that must not end up inside
 * itself, while a single relocation only has to worry about the one item.
 */
export function validateRelocation(
  conflict: Conflict,
  destination: string,
  scope: RelocationScope,
  context: RelocationContext,
): DestinationProblem | null {
  const shape = validateDestinationPath(destination);
  if (shape !== null) return shape;

  const directory = normaliseDirectory(destination);
  if (lookupKind(context.existing, directory) === 'file') {
    return { message: `"${directory}" is a file. A destination has to be a folder.` };
  }

  const groupRoot = rootFor(conflict, context.grouping);
  if (scope === 'group' && groupRoot !== null && groupRoot !== '') {
    if (samePath(directory, groupRoot)) return { message: 'That is where it already is.' };
    if (isInside(directory, groupRoot)) {
      return { message: 'A folder cannot be moved inside itself.' };
    }
  }

  // A folder landing at `Dashboard` cannot be sent to `Dashboard` or to
  // `Dashboard/archive`: both make it its own parent.
  if (conflict.incoming === 'directory' && isAtOrInside(directory, conflict.destination)) {
    return { message: 'A folder cannot be moved into itself or one of its own folders.' };
  }

  if (samePath(directory, parentOf(conflict.destination))) {
    return { message: 'That is where it already is.' };
  }

  const wanted = previewFinalPath(conflict, directory, scope, context.grouping);
  const clash = context.items.find(
    (item) => item.source !== conflict.source && samePath(item.destination, wanted),
  );
  if (clash) {
    return { message: `Another item in this import already lands at "${wanted}".` };
  }

  return null;
}

// --------------------------------------------------------------------- scoping

/**
 * Which conflicts a request touches.
 *
 * The scopes are deliberately about the *plan* rather than about a selection:
 * "everything going to the same place" and "everything from this group" are the
 * two questions a user actually has when several things collide at once, and
 * both are answerable without asking them to tick fifteen boxes.
 */
export function targetsOf(
  conflicts: Conflict[],
  request: RelocationRequest,
  grouping: PlanGrouping,
): string[] {
  const anchor = conflicts.find((conflict) => conflict.id === request.conflictId);
  if (!anchor) return [];

  switch (request.scope) {
    case 'one':
      return [anchor.id];
    case 'group': {
      const groupId = grouping.groupOf[anchor.source];
      if (groupId === undefined) return [anchor.id];
      return conflicts
        .filter((conflict) => grouping.groupOf[conflict.source] === groupId)
        .map((conflict) => conflict.id);
    }
    case 'same-destination': {
      const directory = parentOf(anchor.destination);
      return conflicts
        .filter((conflict) => samePath(parentOf(conflict.destination), directory))
        .map((conflict) => conflict.id);
    }
    case 'all':
      return conflicts.map((conflict) => conflict.id);
  }
}

export interface RelocationPlan {
  /** The planned items with the new destinations applied. */
  items: PlannedItem[];
  /** Conflicts whose destination actually changed, by id. */
  moved: string[];
  /** Conflicts the scope selected but which could not take the destination. */
  refused: { id: string; message: string }[];
  /** Source path to its new landing path, for the preview. */
  preview: Record<string, string>;
}

/** One subtree to move, and the sources that belong to it. */
interface Rebase {
  oldRoot: string;
  newRoot: string;
  members: Set<string>;
}

/**
 * Work out the new plan, without applying anything the batch cannot take.
 *
 * A scope that catches an incompatible conflict does not fail the whole
 * request: the compatible ones move and the rest are reported, which is what
 * "apply to all compatible conflicts" has to mean if it is to be useful.
 *
 * Membership is explicit rather than inferred from the path. An unwrapped group
 * has an empty root — its children land at the project root — and every path in
 * the batch is "under" an empty root, so inferring would drag unrelated groups
 * along with it.
 */
export function planRelocation(
  conflicts: Conflict[],
  request: RelocationRequest,
  context: RelocationContext,
): RelocationPlan {
  const directory = normaliseDirectory(request.destination);
  const targets = targetsOf(conflicts, request, context.grouping);
  const byId = new Map(conflicts.map((conflict) => [conflict.id, conflict]));

  const moved: string[] = [];
  const refused: { id: string; message: string }[] = [];
  const rebases: Rebase[] = [];
  const preview: Record<string, string> = {};

  for (const id of targets) {
    const conflict = byId.get(id);
    if (!conflict) continue;

    const problem = validateRelocation(conflict, directory, request.scope, context);
    if (problem !== null) {
      refused.push({ id, message: problem.message });
      continue;
    }

    const groupId = context.grouping.groupOf[conflict.source];
    const groupRoot = rootFor(conflict, context.grouping);

    if (request.scope === 'group' && groupId !== undefined && groupRoot !== null) {
      // The whole group moves as a unit, so its root is what is rebased and
      // every member follows with its own path below the root intact.
      const members = new Set(
        context.items
          .filter((item) => context.grouping.groupOf[item.source] === groupId)
          .map((item) => item.source),
      );
      if (!rebases.some((entry) => entry.oldRoot === groupRoot && entry.newRoot === directory)) {
        rebases.push({ oldRoot: groupRoot, newRoot: directory, members });
      }
    } else {
      const oldRoot = conflict.destination;
      const newRoot = joinRelative(directory, baseNameOf(conflict.destination));
      const members = new Set(
        context.items
          .filter((item) => isAtOrInside(item.destination, oldRoot))
          .map((item) => item.source),
      );
      rebases.push({ oldRoot, newRoot, members });
    }

    moved.push(id);
  }

  const items = context.items.map((item) => {
    for (const entry of rebases) {
      if (!entry.members.has(item.source)) continue;
      const next = rebase(item.destination, entry.oldRoot, entry.newRoot);
      if (next === null || next === item.destination) continue;
      preview[item.source] = next;
      return { ...item, destination: next };
    }
    return item;
  });

  return { items, moved, refused, preview };
}

/**
 * What a relocation would be called, before it is applied.
 *
 * A single item keeps its own name under the new directory; a group keeps its
 * root and everything under it follows. Shown in the dialog so the user is
 * never asked to confirm a path they have not seen.
 */
export function previewFinalPath(
  conflict: Conflict,
  destination: string,
  scope: RelocationScope,
  grouping: PlanGrouping,
): string {
  const directory = normaliseDirectory(destination);
  const groupRoot = rootFor(conflict, grouping);
  if (scope === 'group' && groupRoot !== null) {
    const next = rebase(conflict.destination, groupRoot, directory);
    if (next !== null) return next;
  }
  return joinRelative(directory, baseNameOf(conflict.destination));
}

// ------------------------------------------------------------------- decisions

/**
 * Carry decisions across a re-analysis.
 *
 * A decision is about a specific collision. When the destination moves, the
 * collision it answered is not the collision that is there now — "replace" was
 * agreed about a different file, and honouring it would overwrite something the
 * user never saw. So a moved conflict loses its decision, and one that did not
 * move keeps it.
 *
 * Conflicts that vanished entirely need no decision, and ones that appeared
 * start unresolved, which is what stops the dialog being confirmable.
 */
export function preserveDecisions(
  previous: Decisions,
  before: Conflict[],
  after: Conflict[],
  invalidated: Iterable<string> = [],
): Decisions {
  const wasAt = new Map(before.map((conflict) => [conflict.id, conflict.destination]));
  const dropped = new Set(invalidated);
  const kept: Decisions = {};

  for (const conflict of after) {
    if (dropped.has(conflict.id)) continue;
    const decision = previous[conflict.id];
    if (decision === undefined) continue;
    const previousDestination = wasAt.get(conflict.id);
    if (previousDestination === undefined) continue;
    if (!samePath(previousDestination, conflict.destination)) continue;
    kept[conflict.id] = decision;
  }

  return kept;
}

/**
 * Directories a picker can offer, from what the plan and the project know.
 *
 * The project root is not in the list: it is offered as its own control,
 * because a blank path is refused and `''` cannot be typed.
 */
export function pickableDirectories(existing: Iterable<string>, extra: Iterable<string>): string[] {
  const seen = new Set<string>();
  const directories: string[] = [];
  for (const path of [...extra, ...existing]) {
    const directory = normaliseDirectory(path);
    const key = normalisePath(directory);
    if (directory === '' || seen.has(key)) continue;
    seen.add(key);
    directories.push(directory);
  }
  directories.sort((left, right) => left.localeCompare(right));
  return directories;
}

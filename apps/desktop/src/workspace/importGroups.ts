/**
 * The import, as something the user can rearrange before it happens.
 *
 * A drop becomes groups — detected projects, ordinary folders, loose files —
 * and every group carries where it will land and whether its own folder comes
 * with it. Editing a group changes the *plan*; nothing on disk moves until the
 * plan is executed, which is the property that makes the preview safe to
 * fiddle with.
 *
 * Pure and tested, because the failure here is silent: a plan that quietly
 * merges two projects still imports successfully and is only discovered later,
 * when neither of them runs.
 */
import type { ImportCandidate } from '../api';
import { childPath } from './tabs';

export type GroupKind = 'project' | 'folder' | 'files';

/** How a group's contents are laid out at the destination. */
export type GroupLayout =
  /** The folder's contents land in the destination; the folder does not. */
  | 'unwrap'
  /** The folder lands in the destination, name and all. */
  | 'keep';

/** One thing being imported, as the preview lists it. */
export interface GroupEntry {
  /** The absolute path on this machine. The identity of an entry. */
  path: string;
  name: string;
  isDirectory: boolean;
}

export interface ImportGroup {
  id: string;
  kind: GroupKind;
  /** Editable. For a kept folder this is the name it lands under. */
  name: string;
  entries: GroupEntry[];
  /** Excluded groups stay visible but are not imported. */
  include: boolean;
  layout: GroupLayout;
  /** Project-relative directory. Empty means the project root. */
  destination: string;
  detection?: {
    score: number;
    signals: string[];
    ecosystem: string | null;
    isMonorepo: boolean;
    /** Why it was classified this way, in words. */
    explanation: string;
    /** Projects inside this one that stay part of it. */
    members: string[];
  };
  /** False once the user has changed anything about this group. */
  auto: boolean;
}

/** Loose files are collected under one group with a fixed id. */
export const STANDALONE_ID = 'standalone';

/**
 * Turn what the core found into the starting plan.
 *
 * Each detected project is its own group. Ordinary folders are their own
 * groups too — merging them would be a decision nobody asked for. Loose files
 * are collected into one clearly labelled group rather than being mixed into a
 * project, which is the rule that stops `notes.txt` disappearing into someone's
 * source tree.
 */
export function groupsFrom(candidates: ImportCandidate[], destination: string): ImportGroup[] {
  const groups: ImportGroup[] = [];

  for (const candidate of candidates.filter((item) => item.isDirectory && item.isProject)) {
    // A monorepo's members are listed but stay inside it: splitting them out
    // would break the workspace that ties them together.
    const members = candidate.nested
      .filter((nested) => nested.belongsToWorkspace)
      .map((nested) => nested.relative);

    groups.push({
      id: candidate.path,
      kind: 'project',
      name: candidate.name,
      entries: [{ path: candidate.path, name: candidate.name, isDirectory: true }],
      include: true,
      // Only sensible when exactly one project is coming in; `planFrom` is what
      // enforces that, so the default here is per-group and the caller narrows.
      layout: 'unwrap',
      destination,
      detection: {
        score: candidate.score,
        signals: candidate.signals,
        ecosystem: candidate.ecosystem,
        isMonorepo: candidate.isMonorepo,
        explanation: explain(candidate),
        members,
      },
      auto: true,
    });
  }

  for (const candidate of candidates.filter((item) => item.isDirectory && !item.isProject)) {
    groups.push({
      id: candidate.path,
      kind: 'folder',
      name: candidate.name,
      entries: [{ path: candidate.path, name: candidate.name, isDirectory: true }],
      include: true,
      layout: 'keep',
      destination,
      detection: {
        score: candidate.score,
        signals: candidate.signals,
        ecosystem: candidate.ecosystem,
        isMonorepo: false,
        explanation: explain(candidate),
        members: [],
      },
      auto: true,
    });
  }

  const files = candidates.filter((item) => !item.isDirectory);
  if (files.length > 0) {
    groups.push({
      id: STANDALONE_ID,
      kind: 'files',
      name: 'Standalone files',
      entries: files.map((file) => ({ path: file.path, name: file.name, isDirectory: false })),
      include: true,
      layout: 'unwrap',
      destination,
      auto: true,
    });
  }

  return normaliseLayouts(groups);
}

/**
 * Only one group may unwrap into a given destination.
 *
 * Two projects both claiming the root would interleave their files, which is
 * the one outcome that cannot be undone by moving things afterwards. When
 * several want the same place they all keep their folders instead.
 */
export function normaliseLayouts(groups: ImportGroup[]): ImportGroup[] {
  const unwrappingPerDestination = new Map<string, number>();
  for (const group of groups) {
    if (!group.include || group.kind === 'files' || group.layout !== 'unwrap') continue;
    unwrappingPerDestination.set(
      group.destination,
      (unwrappingPerDestination.get(group.destination) ?? 0) + 1,
    );
  }

  return groups.map((group) => {
    if (group.kind === 'files' || group.layout !== 'unwrap' || !group.include) return group;
    const clash = (unwrappingPerDestination.get(group.destination) ?? 0) > 1;
    return clash ? { ...group, layout: 'keep' } : group;
  });
}

function explain(candidate: ImportCandidate): string {
  if (candidate.signals.length === 0) {
    return 'No project markers were found, so it is treated as an ordinary folder.';
  }
  const shown = candidate.signals.slice(0, 6).join(', ');
  const more = candidate.signals.length > 6 ? `, and ${candidate.signals.length - 6} more` : '';
  const kind = candidate.ecosystem ? `${candidate.ecosystem} project` : 'project';
  const workspace = candidate.isMonorepo ? ' It holds several packages.' : '';

  return candidate.isProject
    ? `Detected as a ${kind} from ${shown}${more} (score ${candidate.score}).${workspace}`
    : `Found ${shown}${more} (score ${candidate.score}), which is not enough to call it a project.`;
}

// ------------------------------------------------------------------- editing

function patch(
  groups: ImportGroup[],
  id: string,
  change: (group: ImportGroup) => ImportGroup,
): ImportGroup[] {
  return groups.map((group) => (group.id === id ? { ...change(group), auto: false } : group));
}

export function setInclude(groups: ImportGroup[], id: string, include: boolean): ImportGroup[] {
  return normaliseLayouts(patch(groups, id, (group) => ({ ...group, include })));
}

export function renameGroup(groups: ImportGroup[], id: string, name: string): ImportGroup[] {
  return patch(groups, id, (group) => ({ ...group, name }));
}

export function setLayout(groups: ImportGroup[], id: string, layout: GroupLayout): ImportGroup[] {
  return normaliseLayouts(patch(groups, id, (group) => ({ ...group, layout })));
}

export function setDestination(
  groups: ImportGroup[],
  id: string,
  destination: string,
): ImportGroup[] {
  return normaliseLayouts(patch(groups, id, (group) => ({ ...group, destination })));
}

/**
 * Reclassify a group.
 *
 * A folder the heuristic missed becomes a project; a project it was too eager
 * about becomes a folder. The layout follows, because that is the difference
 * the two kinds actually make.
 */
export function setKind(groups: ImportGroup[], id: string, kind: GroupKind): ImportGroup[] {
  return normaliseLayouts(
    patch(groups, id, (group) => ({
      ...group,
      kind,
      layout: kind === 'project' ? 'unwrap' : 'keep',
    })),
  );
}

/** Put a group back to what detection recommended. */
export function resetGroup(
  groups: ImportGroup[],
  id: string,
  candidates: ImportCandidate[],
  destination: string,
): ImportGroup[] {
  const fresh = groupsFrom(candidates, destination).find((group) => group.id === id);
  if (!fresh) return groups;
  return normaliseLayouts(groups.map((group) => (group.id === id ? fresh : group)));
}

/**
 * Move entries from one group to another.
 *
 * This is a change to the plan only. The files on this machine are not touched
 * — nothing is until the plan runs.
 */
export function moveEntries(
  groups: ImportGroup[],
  fromId: string,
  toId: string,
  paths: string[],
): ImportGroup[] {
  if (fromId === toId) return groups;
  const source = groups.find((group) => group.id === fromId);
  if (!source) return groups;

  const moving = source.entries.filter((entry) => paths.includes(entry.path));
  if (moving.length === 0) return groups;

  const next = groups.map((group) => {
    if (group.id === fromId) {
      return {
        ...group,
        entries: group.entries.filter((entry) => !paths.includes(entry.path)),
        auto: false,
      };
    }
    if (group.id === toId) {
      const existing = new Set(group.entries.map((entry) => entry.path));
      return {
        ...group,
        entries: [...group.entries, ...moving.filter((entry) => !existing.has(entry.path))],
        auto: false,
      };
    }
    return group;
  });

  // A group with nothing left in it is not a group.
  return normaliseLayouts(next.filter((group) => group.entries.length > 0));
}

/** Drop entries from the import entirely. */
export function removeEntries(groups: ImportGroup[], paths: string[]): ImportGroup[] {
  const next = groups.map((group) => ({
    ...group,
    entries: group.entries.filter((entry) => !paths.includes(entry.path)),
    auto: group.entries.some((entry) => paths.includes(entry.path)) ? false : group.auto,
  }));
  return normaliseLayouts(next.filter((group) => group.entries.length > 0));
}

/** A new, empty group the user can drag entries into. */
export function createGroup(groups: ImportGroup[], name: string, kind: GroupKind): ImportGroup[] {
  const id = `custom-${groups.length}-${name}`;
  const destination = groups[0]?.destination ?? '';
  return [
    ...groups,
    {
      id,
      kind,
      name,
      entries: [],
      include: true,
      layout: kind === 'project' ? 'unwrap' : 'keep',
      destination,
      auto: false,
    },
  ];
}

/**
 * Merge several groups into the first of them.
 *
 * Only ever on request. Two projects are never combined automatically, because
 * a merge cannot be undone once their files are interleaved.
 */
export function mergeGroups(groups: ImportGroup[], ids: string[]): ImportGroup[] {
  if (ids.length < 2) return groups;
  const targetId = ids[0]!;
  const target = groups.find((group) => group.id === targetId);
  if (!target) return groups;

  const absorbed = groups.filter((group) => ids.includes(group.id) && group.id !== targetId);
  const seen = new Set(target.entries.map((entry) => entry.path));
  const entries = [...target.entries];
  for (const group of absorbed) {
    for (const entry of group.entries) {
      if (!seen.has(entry.path)) {
        seen.add(entry.path);
        entries.push(entry);
      }
    }
  }

  return normaliseLayouts(
    groups
      .filter((group) => !ids.includes(group.id) || group.id === targetId)
      .map((group) =>
        group.id === targetId
          ? // A merged group holds several things, so it can no longer unwrap
            // one of them over the destination.
            { ...group, entries, layout: 'keep' as const, auto: false }
          : group,
      ),
  );
}

// ------------------------------------------------------------------ planning

/** What the import command needs, derived from the approved groups. */
export interface ExecutionPlan {
  /** One call per destination directory. */
  batches: {
    destination: string;
    sourcePaths: string[];
    unwrapPaths: string[];
  }[];
  /** Every top-level path the import will create, for conflict checking. */
  destinations: { path: string; from: string }[];
  excluded: number;
}

/**
 * Turn the groups into the calls that will be made.
 *
 * Grouped by destination because the core imports into one directory at a time,
 * and one call per destination keeps a single staging directory and a single
 * rollback per batch.
 */
export function planFrom(groups: ImportGroup[]): ExecutionPlan {
  const byDestination = new Map<string, { sourcePaths: string[]; unwrapPaths: string[] }>();
  const destinations: { path: string; from: string }[] = [];
  let excluded = 0;

  for (const group of groups) {
    if (!group.include || group.entries.length === 0) {
      excluded += group.entries.length;
      continue;
    }

    const batch = byDestination.get(group.destination) ?? { sourcePaths: [], unwrapPaths: [] };
    const unwrapping = group.kind !== 'files' && group.layout === 'unwrap';

    for (const entry of group.entries) {
      batch.sourcePaths.push(entry.path);
      if (unwrapping && entry.isDirectory) {
        batch.unwrapPaths.push(entry.path);
        // Unwrapping lands the folder's children, whose names are not known
        // here; the conflict check uses what the core reported instead.
      } else {
        // A kept folder lands under the group's name when the group was
        // renamed, and under its own otherwise.
        const landing =
          group.kind !== 'files' && group.entries.length === 1 ? group.name : entry.name;
        destinations.push({ path: childPath(group.destination, landing), from: entry.path });
      }
    }

    byDestination.set(group.destination, batch);
  }

  return {
    batches: [...byDestination.entries()].map(([destination, batch]) => ({
      destination,
      ...batch,
    })),
    destinations,
    excluded,
  };
}

/**
 * Which of the plan's destinations already exist.
 *
 * Run immediately before executing as well as while previewing: the project's
 * files can change while a preview is open, and a conflict check from thirty
 * seconds ago is not a conflict check.
 */
export function conflictsWith(plan: ExecutionPlan, existing: string[]): string[] {
  const taken = new Set(existing.map((path) => path.toLowerCase()));
  return plan.destinations
    .map((destination) => destination.path)
    .filter((path) => taken.has(path.toLowerCase()));
}

/** A one-line summary of what will happen. */
export function summarise(groups: ImportGroup[]): string {
  const included = groups.filter((group) => group.include && group.entries.length > 0);
  const projects = included.filter((group) => group.kind === 'project').length;
  const folders = included.filter((group) => group.kind === 'folder').length;
  const files = included
    .filter((group) => group.kind === 'files')
    .reduce((total, group) => total + group.entries.length, 0);

  const parts: string[] = [];
  if (projects > 0) parts.push(`${projects} project${projects === 1 ? '' : 's'}`);
  if (folders > 0) parts.push(`${folders} folder${folders === 1 ? '' : 's'}`);
  if (files > 0) parts.push(`${files} file${files === 1 ? '' : 's'}`);

  if (parts.length === 0) return 'Nothing selected to import.';
  if (parts.length === 1) return `Importing ${parts[0]}.`;
  return `Importing ${parts.slice(0, -1).join(', ')} and ${parts[parts.length - 1]}.`;
}

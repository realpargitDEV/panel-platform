/**
 * Deciding what to do about destinations that are already taken.
 *
 * One model for every operation that can collide — paste, drag, move, import —
 * so there is one dialog, one set of choices and one place where "replace"
 * means something specific.
 *
 * The whole batch is analysed before anything is written. Resolving conflicts
 * one at a time during execution means the user answers question four with
 * three files already overwritten, and cancelling then leaves the operation
 * half done.
 */

export type Operation = 'copy' | 'move' | 'import';

/** What is in the way, and what is arriving. */
export type ItemKind = 'file' | 'directory';

export type ConflictKind =
  /** Two files. Only a replace, a rename or a skip can resolve it. */
  | 'file-over-file'
  /** A folder arriving where a folder already is. Merging is safe. */
  | 'directory-over-directory'
  /** A file arriving where a folder is. Never merged. */
  | 'file-over-directory'
  /** A folder arriving where a file is. Never merged. */
  | 'directory-over-file';

export type Resolution = 'replace' | 'skip' | 'rename' | 'merge' | 'unresolved';

export interface Conflict {
  /** Stable across a re-analysis, so decisions survive revalidation. */
  id: string;
  source: string;
  /** Where it wanted to land. */
  destination: string;
  kind: ConflictKind;
  incoming: ItemKind;
  existing: ItemKind;
  operation: Operation;
  /** The choices that make sense for this kind. */
  allowed: Resolution[];
  /** True when a generated name would resolve it without asking anything else. */
  renameable: boolean;
}

export interface Decision {
  resolution: Resolution;
  /** Only for `rename`: the path the item will actually take. */
  renamedTo?: string;
}

export type Decisions = Record<string, Decision>;

/** What the caller wants to do, before conflicts are known. */
export interface PlannedItem {
  source: string;
  destination: string;
  incoming: ItemKind;
}

export function conflictKind(incoming: ItemKind, existing: ItemKind): ConflictKind {
  if (incoming === 'directory' && existing === 'directory') return 'directory-over-directory';
  if (incoming === 'file' && existing === 'file') return 'file-over-file';
  if (incoming === 'file' && existing === 'directory') return 'file-over-directory';
  return 'directory-over-file';
}

/**
 * Which resolutions a conflict can take.
 *
 * Merging is offered only for two directories: a file cannot be merged into a
 * folder, and pretending otherwise would mean deleting one of them.
 */
export function allowedFor(kind: ConflictKind): Resolution[] {
  return kind === 'directory-over-directory'
    ? ['merge', 'rename', 'skip', 'replace']
    : ['replace', 'rename', 'skip'];
}

/**
 * Compare paths the way the filesystem does.
 *
 * Windows and a default macOS volume are case-insensitive, so `README.md` and
 * `readme.md` are one file. Comparing them as distinct is how an "overwrite"
 * happens without a conflict ever being reported. Separators are normalised
 * too: the core speaks forward slashes but a path can arrive either way.
 */
export function normalisePath(path: string): string {
  return path.replace(/\\/g, '/').replace(/\/+$/, '').toLowerCase();
}

export function samePath(left: string, right: string): boolean {
  return normalisePath(left) === normalisePath(right);
}

/**
 * A name that is free.
 *
 * `config.json` becomes `config copy.json`, then `config copy 2.json`. A
 * directory has no extension to preserve, so `src` becomes `src copy`.
 *
 * `taken` must include the paths this batch is *planning* to create as well as
 * the ones already on disk — two incoming files that both want `a.txt` need
 * two different names, not the same one twice.
 */
export function generateName(destination: string, taken: Iterable<string>): string {
  const used = new Set([...taken].map(normalisePath));
  if (!used.has(normalisePath(destination))) return destination;

  const cut = destination.lastIndexOf('/');
  const directory = cut < 0 ? '' : destination.slice(0, cut + 1);
  const name = destination.slice(cut + 1);
  const dot = name.lastIndexOf('.');
  // A leading dot is part of the name, not an extension: `.env` must not
  // become ` copy.env`.
  const stem = dot > 0 ? name.slice(0, dot) : name;
  const extension = dot > 0 ? name.slice(dot) : '';

  for (let counter = 1; ; counter += 1) {
    const suffix = counter === 1 ? ' copy' : ` copy ${counter}`;
    const candidate = `${directory}${stem}${suffix}${extension}`;
    if (!used.has(normalisePath(candidate))) return candidate;
  }
}

export interface Analysis {
  conflicts: Conflict[];
  /** Items with nowhere to collide. Carried through untouched. */
  clear: PlannedItem[];
  /** Two incoming items wanting one destination, which is not the disk's fault. */
  internal: Conflict[];
}

/**
 * Work out every collision before anything is written.
 *
 * `existing` is what is already at the destination, with the kind of each — a
 * file arriving where a folder sits is a different problem from two files, and
 * offering "merge" for it would be wrong.
 */
export function analyse(
  items: PlannedItem[],
  existing: Map<string, ItemKind>,
  operation: Operation,
): Analysis {
  const onDisk = new Map<string, ItemKind>();
  for (const [path, kind] of existing) onDisk.set(normalisePath(path), kind);

  const conflicts: Conflict[] = [];
  const internal: Conflict[] = [];
  const clear: PlannedItem[] = [];
  const claimed = new Map<string, PlannedItem>();

  for (const item of items) {
    const key = normalisePath(item.destination);

    // Another item in this same batch already wants that path. The filesystem
    // has nothing to say about it, and it has to be resolved all the same.
    const rival = claimed.get(key);
    if (rival) {
      const kind = conflictKind(item.incoming, rival.incoming);
      internal.push({
        id: `internal:${item.source}`,
        source: item.source,
        destination: item.destination,
        kind,
        incoming: item.incoming,
        existing: rival.incoming,
        operation,
        allowed: ['rename', 'skip'],
        renameable: true,
      });
      continue;
    }
    claimed.set(key, item);

    const existingKind = onDisk.get(key);
    if (existingKind === undefined) {
      clear.push(item);
      continue;
    }

    const kind = conflictKind(item.incoming, existingKind);
    conflicts.push({
      id: `disk:${item.source}`,
      source: item.source,
      destination: item.destination,
      kind,
      incoming: item.incoming,
      existing: existingKind,
      operation,
      allowed: allowedFor(kind),
      renameable: true,
    });
  }

  return { conflicts, clear, internal };
}

/** Every conflict, disk and internal, in the order the dialog shows them. */
export function allConflicts(analysis: Analysis): Conflict[] {
  return [...analysis.conflicts, ...analysis.internal];
}

/**
 * The decision a fresh dialog starts with.
 *
 * Two directories default to merging, which is safe and almost always wanted.
 * Everything else starts unresolved, because the alternatives all either
 * destroy something or silently drop it, and neither should be the default.
 */
export function defaultDecisions(conflicts: Conflict[]): Decisions {
  const decisions: Decisions = {};
  for (const conflict of conflicts) {
    decisions[conflict.id] =
      conflict.kind === 'directory-over-directory'
        ? { resolution: 'merge' }
        : { resolution: 'unresolved' };
  }
  return decisions;
}

/** Apply one resolution to every conflict, or to every one of a kind. */
export function applyToAll(
  conflicts: Conflict[],
  decisions: Decisions,
  resolution: Resolution,
  onlyKind?: ConflictKind,
): Decisions {
  const next = { ...decisions };
  for (const conflict of conflicts) {
    if (onlyKind !== undefined && conflict.kind !== onlyKind) continue;
    if (!conflict.allowed.includes(resolution)) continue;
    next[conflict.id] = { resolution };
  }
  return next;
}

export function setDecision(decisions: Decisions, id: string, resolution: Resolution): Decisions {
  return { ...decisions, [id]: { resolution } };
}

export function unresolvedCount(conflicts: Conflict[], decisions: Decisions): number {
  return conflicts.filter(
    (conflict) => (decisions[conflict.id]?.resolution ?? 'unresolved') === 'unresolved',
  ).length;
}

/** One item to carry out, with its collision settled. */
export interface ResolvedItem {
  source: string;
  /** Where it will actually land, after any rename. */
  destination: string;
  incoming: ItemKind;
  /** True when something has to be moved aside and restored on failure. */
  replaces: boolean;
  /** What is being replaced, so the caller can delete it correctly. */
  replacedDirectory?: boolean;
  /** True when the two directories are being combined rather than swapped. */
  merges: boolean;
}

export interface ResolvedPlan {
  items: ResolvedItem[];
  skipped: string[];
  /** Names the batch generated, so a preview can show the final paths. */
  renames: Record<string, string>;
}

/**
 * Turn decisions into the work to do.
 *
 * Generated names are allocated against the disk *and* against the names this
 * batch has already claimed, so no two items can be sent to one path.
 */
export function resolvePlan(
  analysis: Analysis,
  decisions: Decisions,
  existing: Iterable<string>,
): ResolvedPlan {
  const taken = new Set([...existing].map(normalisePath));
  for (const item of analysis.clear) taken.add(normalisePath(item.destination));

  const items: ResolvedItem[] = analysis.clear.map((item) => ({
    source: item.source,
    destination: item.destination,
    incoming: item.incoming,
    replaces: false,
    merges: false,
  }));
  const skipped: string[] = [];
  const renames: Record<string, string> = {};

  for (const conflict of allConflicts(analysis)) {
    const decision = decisions[conflict.id]?.resolution ?? 'unresolved';

    switch (decision) {
      case 'skip':
      case 'unresolved':
        // An unresolved conflict is never carried out. The dialog refuses to
        // confirm while any remain, so reaching this means a stale plan.
        skipped.push(conflict.source);
        break;
      case 'rename': {
        const destination = generateName(conflict.destination, taken);
        taken.add(normalisePath(destination));
        renames[conflict.id] = destination;
        items.push({
          source: conflict.source,
          destination,
          incoming: conflict.incoming,
          replaces: false,
          merges: false,
        });
        break;
      }
      case 'merge':
        items.push({
          source: conflict.source,
          destination: conflict.destination,
          incoming: conflict.incoming,
          replaces: false,
          merges: true,
        });
        break;
      case 'replace':
        items.push({
          source: conflict.source,
          destination: conflict.destination,
          incoming: conflict.incoming,
          replaces: true,
          replacedDirectory: conflict.existing === 'directory',
          merges: false,
        });
        break;
    }
  }

  return { items, skipped, renames };
}

/**
 * Has the destination changed since the dialog opened?
 *
 * Compared against what analysis saw. A file that appeared while the user was
 * deciding would otherwise be overwritten by a "replace" they agreed to about
 * a different file — or by a "rename" whose generated name is now taken.
 */
export function planIsStale(plan: ResolvedPlan, existingNow: Iterable<string>): boolean {
  const now = new Set([...existingNow].map(normalisePath));
  return plan.items.some(
    (item) => !item.replaces && !item.merges && now.has(normalisePath(item.destination)),
  );
}

/** How a conflict reads in the dialog. */
export function describeConflict(conflict: Conflict): string {
  const incoming = conflict.incoming === 'directory' ? 'folder' : 'file';
  const existing = conflict.existing === 'directory' ? 'folder' : 'file';

  switch (conflict.kind) {
    case 'directory-over-directory':
      return `A folder called ${nameOf(conflict.destination)} is already here. Merging adds the incoming files beside the existing ones.`;
    case 'file-over-file':
      return `A file called ${nameOf(conflict.destination)} is already here.`;
    default:
      return `A ${existing} called ${nameOf(conflict.destination)} is already here, and a ${incoming} is arriving. They cannot be combined.`;
  }
}

function nameOf(path: string): string {
  const cut = path.lastIndexOf('/');
  return cut < 0 ? path : path.slice(cut + 1);
}

/**
 * Copy, cut and paste for files.
 *
 * The planning is here and the execution is in the workspace, so the rules that
 * destroy data when they are wrong — pasting a folder into itself, pasting over
 * a file, removing the source of a cut before the copy succeeded — are decided
 * by a pure function with tests rather than in the middle of an async loop.
 *
 * Nothing here touches the disk. A plan is a list of intentions plus the
 * problems found with them; the caller resolves the conflicts and then acts.
 */
import { childPath, parentOf } from './tabs';

export type ClipboardMode = 'copy' | 'cut';

export interface Clipboard {
  mode: ClipboardMode;
  /** Project-relative paths, as they were when the clipboard was filled. */
  paths: string[];
}

/** One thing to do: copy or move `from` to `to`. */
export interface PasteItem {
  from: string;
  to: string;
  isDirectory: boolean;
}

export interface PasteRejection {
  path: string;
  reason: string;
}

export interface PastePlan {
  mode: ClipboardMode;
  items: PasteItem[];
  /** Destinations that already exist, for the conflict dialog. */
  conflicts: string[];
  /** What was dropped from the plan, and why. */
  rejected: PasteRejection[];
}

/**
 * Is `candidate` inside `ancestor`?
 *
 * The separator check matters: `src` is not an ancestor of `srcfile.ts`, and
 * treating it as one would refuse legitimate pastes.
 */
export function isDescendant(ancestor: string, candidate: string): boolean {
  return candidate === ancestor || candidate.startsWith(`${ancestor}/`);
}

export function nameOf(path: string): string {
  const cut = path.lastIndexOf('/');
  return cut < 0 ? path : path.slice(cut + 1);
}

/**
 * Where a paste should land.
 *
 * Pasting with a single folder selected puts the files in that folder — which
 * is what selecting it and pressing Ctrl+V means. Anything else pastes into the
 * directory the explorer is showing.
 */
export function pasteDestination(
  selected: string[],
  isDirectory: (path: string) => boolean,
  currentDirectory: string,
): string {
  const [only] = selected;
  if (selected.length === 1 && only !== undefined && isDirectory(only)) return only;
  return currentDirectory;
}

/**
 * Work out what a paste would do.
 *
 * `existing` is every path already in the destination directory, used to report
 * conflicts before anything is written.
 */
export function planPaste(
  clipboard: Clipboard,
  destination: string,
  existing: string[],
  isDirectory: (path: string) => boolean,
): PastePlan {
  const items: PasteItem[] = [];
  const rejected: PasteRejection[] = [];
  const conflicts: string[] = [];
  const taken = new Set(existing.map((path) => path.toLowerCase()));
  const claimed = new Set<string>();

  for (const from of clipboard.paths) {
    const directory = isDirectory(from);

    // A folder cannot contain itself. This is the check that prevents an
    // infinite copy, and it has to consider the destination *and* everything
    // below it.
    if (directory && isDescendant(from, destination)) {
      rejected.push({
        path: from,
        reason: `${nameOf(from)} cannot be pasted into itself.`,
      });
      continue;
    }

    const to = childPath(destination, nameOf(from));

    // Pasting something back where it already is does nothing, and the core
    // refuses it. Saying so beats an error from three layers down. Both checks
    // describe the same situation, so they give the same answer.
    if (to === from || (clipboard.mode === 'cut' && parentOf(from) === destination)) {
      rejected.push({
        path: from,
        reason:
          clipboard.mode === 'cut'
            ? `${nameOf(from)} is already in this folder.`
            : `${nameOf(from)} would replace itself; paste it into another folder.`,
      });
      continue;
    }

    const key = to.toLowerCase();
    if (claimed.has(key)) {
      rejected.push({
        path: from,
        reason: `Two of the items being pasted are called ${nameOf(from)}.`,
      });
      continue;
    }
    claimed.add(key);

    if (taken.has(key)) conflicts.push(to);
    items.push({ from, to, isDirectory: directory });
  }

  return { mode: clipboard.mode, items, conflicts, rejected };
}

/**
 * Where a drag should move things.
 *
 * The same rules as a cut-and-paste, reported the same way, because a drag onto
 * a folder *is* a move and should refuse the same nonsense.
 */
export function planMove(
  paths: string[],
  destination: string,
  existing: string[],
  isDirectory: (path: string) => boolean,
): PastePlan {
  return planPaste({ mode: 'cut', paths }, destination, existing, isDirectory);
}

/** A short sentence naming what a plan will do, for the transcript. */
export function describePaste(plan: PastePlan, destination: string): string {
  const where = destination || 'the project root';
  const verb = plan.mode === 'cut' ? 'Moving' : 'Copying';
  if (plan.items.length === 0) return `Nothing to paste into ${where}.`;
  return `${verb} ${plan.items.length} item${plan.items.length === 1 ? '' : 's'} into ${where}.`;
}

/**
 * What to do when a dropped file is already there.
 *
 * The core refuses to overwrite: `begin_upload` fails with "already exists"
 * rather than replacing a file the user did not mean to lose. That is the right
 * default, but on its own it turns a drop of forty files into forty errors, so
 * the explorer asks first and this module works out what it is asking about.
 *
 * "Replace" is not a silent overwrite either — the workspace deletes the
 * existing entry through the normal command and then uploads, so the same
 * guards apply.
 */

/** What the user chose in the conflict dialog. */
export type ConflictChoice = 'replace' | 'rename' | 'skip';

export interface ConflictCandidate {
  /** Where this upload wants to land, relative to the project root. */
  path: string;
}

/**
 * Which of these paths already exist.
 *
 * Compared case-insensitively: on Windows and on a default macOS volume
 * `README.md` and `readme.md` are the same file, and reporting no conflict
 * there would send the user straight into the failure this exists to prevent.
 */
export function conflictingPaths(existing: string[], candidates: string[]): string[] {
  const taken = new Set(existing.map((path) => path.toLowerCase()));
  const seen = new Set<string>();
  const conflicts: string[] = [];

  for (const candidate of candidates) {
    const key = candidate.toLowerCase();
    if (taken.has(key) && !seen.has(key)) {
      seen.add(key);
      conflicts.push(candidate);
    }
  }
  return conflicts;
}

/** A file name split at the extension. `.gitignore` is all stem, no extension. */
export function splitName(name: string): { stem: string; extension: string } {
  const dot = name.lastIndexOf('.');
  if (dot <= 0) return { stem: name, extension: '' };
  return { stem: name.slice(0, dot), extension: name.slice(dot) };
}

/**
 * A path that is free, derived from one that is not.
 *
 * `notes.txt` becomes `notes 1.txt`, then `notes 2.txt`. The counter is not
 * capped: a caller looping forever would need to have created every name up to
 * the loop bound, which means the loop bound is the bug.
 */
export function availablePath(existing: string[], path: string): string {
  const taken = new Set(existing.map((entry) => entry.toLowerCase()));
  if (!taken.has(path.toLowerCase())) return path;

  const cut = path.lastIndexOf('/');
  const directory = cut < 0 ? '' : path.slice(0, cut + 1);
  const { stem, extension } = splitName(path.slice(cut + 1));

  for (let counter = 1; ; counter += 1) {
    const candidate = `${directory}${stem} ${counter}${extension}`;
    if (!taken.has(candidate.toLowerCase())) return candidate;
  }
}

export interface ResolvedUpload<T extends ConflictCandidate> {
  item: T;
  /** Where it will actually be written. Differs from `item.path` after a rename. */
  path: string;
  /** True when the existing entry has to be deleted first. */
  replaces: boolean;
}

export interface Resolution<T extends ConflictCandidate> {
  uploads: ResolvedUpload<T>[];
  skipped: T[];
}

/**
 * Turn a set of candidates and one choice into the work to do.
 *
 * Renames are resolved against the names *this batch* is claiming as well as
 * the ones already on disk, so dropping two copies of `notes.txt` produces
 * `notes 1.txt` and `notes 2.txt` rather than two attempts at the same name.
 */
export function resolveConflicts<T extends ConflictCandidate>(
  existing: string[],
  candidates: T[],
  choice: ConflictChoice,
): Resolution<T> {
  const taken = [...existing];
  const uploads: ResolvedUpload<T>[] = [];
  const skipped: T[] = [];

  for (const item of candidates) {
    const clashes = taken.some((path) => path.toLowerCase() === item.path.toLowerCase());
    if (!clashes) {
      taken.push(item.path);
      uploads.push({ item, path: item.path, replaces: false });
      continue;
    }

    switch (choice) {
      case 'skip':
        skipped.push(item);
        break;
      case 'replace':
        uploads.push({ item, path: item.path, replaces: true });
        break;
      case 'rename': {
        const path = availablePath(taken, item.path);
        taken.push(path);
        uploads.push({ item, path, replaces: false });
        break;
      }
    }
  }

  return { uploads, skipped };
}

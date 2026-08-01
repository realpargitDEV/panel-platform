/**
 * Which projects were opened recently.
 *
 * Kept in the window's own storage rather than the database: it is a property
 * of this person at this desk, not of the project, and the core has no reason
 * to know about it. Ids only — the names come from the live project list, so a
 * renamed project shows its new name and a deleted one disappears.
 */

const KEY = 'panel.recentProjects.v1';
const LIMIT = 6;

export interface RecentStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

/** Most recent first, ids only, never longer than the limit. */
export function readRecent(storage: RecentStorage | undefined): string[] {
  const raw = storage?.getItem(KEY);
  if (!raw) return [];
  try {
    const parsed: unknown = JSON.parse(raw);
    if (!Array.isArray(parsed)) return [];
    return parsed.filter((item): item is string => typeof item === 'string').slice(0, LIMIT);
  } catch {
    return [];
  }
}

/**
 * Record an open, moving it to the front.
 *
 * Returns the new list rather than only writing it, so the caller can render
 * without reading storage back.
 */
export function recordRecent(storage: RecentStorage | undefined, id: string): string[] {
  const next = [id, ...readRecent(storage).filter((entry) => entry !== id)].slice(0, LIMIT);
  try {
    storage?.setItem(KEY, JSON.stringify(next));
  } catch {
    // A storage that refuses is not worth an error in front of anyone.
  }
  return next;
}

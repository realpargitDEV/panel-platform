/**
 * The command palette's contents, and how a query narrows them.
 *
 * The registry is built by the workspace from callbacks it already has, so
 * every entry runs something real: there is no table of names here waiting for
 * a feature to exist. A command that cannot run right now — save with nothing
 * open, stop a project that is not running — is listed with `enabled: false`
 * and says why, which is more useful than hiding it.
 *
 * The matcher is a subsequence match, the same shape VS Code uses: typing
 * "tglsb" finds "Toggle Sidebar". Kept pure and tested here because ranking is
 * the part that quietly stops working.
 */

export type CommandId = string;

export interface Command {
  id: CommandId;
  /** What the palette shows, e.g. "Toggle Sidebar". */
  title: string;
  /** The grey prefix, e.g. "View". */
  category: string;
  /** Rendered on the right, e.g. "Ctrl+B". Display only. */
  keybinding?: string;
  /** False when the command exists but cannot run now; `reason` says why. */
  enabled?: boolean;
  reason?: string;
  run: () => void;
}

export interface Match<T> {
  item: T;
  /** Indices of the characters that matched, for highlighting. */
  positions: number[];
  score: number;
}

/**
 * Does `query` appear in `text` as a subsequence, and how well?
 *
 * Returns null for no match. A higher score is better. Consecutive characters
 * and matches at a word boundary score more, which is what makes "np" rank
 * "New Project" above "Open Panel".
 */
export function fuzzyMatch(
  text: string,
  query: string,
): { positions: number[]; score: number } | null {
  if (query.length === 0) return { positions: [], score: 0 };

  const haystack = text.toLowerCase();
  const needle = query.toLowerCase();
  const positions: number[] = [];
  let score = 0;
  let cursor = 0;

  for (const character of needle) {
    const found = haystack.indexOf(character, cursor);
    if (found < 0) return null;

    if (found === 0) {
      score += 12;
    } else if (positions.length > 0 && found === positions[positions.length - 1]! + 1) {
      score += 8;
    } else if (isBoundary(haystack, found)) {
      score += 6;
    } else {
      score += 1;
    }

    positions.push(found);
    cursor = found + 1;
  }

  // A short name that matched every character is a better answer than a long
  // one that happened to contain the same letters.
  return { positions, score: score - Math.floor(haystack.length / 12) };
}

/**
 * Rank commands against a query.
 *
 * Matching runs against "Category: Title" so typing a category name works.
 * Positions come back relative to the title alone, because that is what the
 * palette highlights.
 */
export function matchCommands(commands: Command[], query: string): Match<Command>[] {
  const trimmed = query.trim();
  if (trimmed.length === 0) {
    return commands.map((item) => ({ item, positions: [], score: 0 }));
  }

  const matches: Match<Command>[] = [];
  for (const item of commands) {
    const onTitle = fuzzyMatch(item.title, trimmed);
    if (onTitle) {
      matches.push({ item, positions: onTitle.positions, score: onTitle.score + 20 });
      continue;
    }
    // A weaker match: the query only lines up once the category is included.
    const onFull = fuzzyMatch(`${item.category} ${item.title}`, trimmed);
    if (onFull) matches.push({ item, positions: [], score: onFull.score });
  }

  return matches.sort(byScoreThenTitle);
}

/**
 * Rank paths for quick open.
 *
 * The file name is matched first and scored above the directories, so "index"
 * offers `src/index.ts` before `src/index/other.ts` — the file whose *name* the
 * user typed, not the one that merely lives under a folder of that name.
 */
export function matchPaths(paths: string[], query: string, limit = 50): Match<string>[] {
  const trimmed = query.trim();
  if (trimmed.length === 0) {
    return paths.slice(0, limit).map((item) => ({ item, positions: [], score: 0 }));
  }

  const matches: Match<string>[] = [];
  for (const item of paths) {
    const name = item.slice(item.lastIndexOf('/') + 1);
    const onName = fuzzyMatch(name, trimmed);
    if (onName) {
      matches.push({ item, positions: onName.positions, score: onName.score + 20 });
      continue;
    }
    const onPath = fuzzyMatch(item, trimmed);
    if (onPath) matches.push({ item, positions: [], score: onPath.score });
  }

  return matches.sort(byScoreThenItem).slice(0, limit);
}

function byScoreThenTitle(left: Match<Command>, right: Match<Command>): number {
  return right.score - left.score || left.item.title.localeCompare(right.item.title);
}

function byScoreThenItem(left: Match<string>, right: Match<string>): number {
  return right.score - left.score || left.item.localeCompare(right.item);
}

function isBoundary(text: string, index: number): boolean {
  const previous = text[index - 1];
  return (
    previous === ' ' || previous === '-' || previous === '_' || previous === '/' || previous === '.'
  );
}

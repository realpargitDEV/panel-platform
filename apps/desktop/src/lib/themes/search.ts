/**
 * Finding a theme among eighty-one.
 *
 * A flat grid of eighty-one cards is a list nobody reads to the end, so the
 * browser has a search box and a category filter, and this is what they run.
 * Pure, and separate from the component, so the matching rules can be tested as
 * arithmetic rather than by typing into a rendered input.
 */

import { CATEGORY_BY_ID } from './categories';
import type { CategoryId, Theme } from './types';

export interface ThemeQuery {
  /** Free text. Empty matches everything. */
  text?: string;
  /** `undefined` means every category. */
  category?: CategoryId;
}

/** Lower-cased, collapsed, and with the punctuation people leave out. */
function normalise(value: string): string {
  return value
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, ' ')
    .trim();
}

/**
 * Match a theme against a search term.
 *
 * Name, detail and category name are all searchable: someone typing "green"
 * means the description, someone typing "retro" means the group, and someone
 * typing "vapor" means the name. Term-by-term rather than as one string, so
 * "dark green" finds a theme whose name says dark and whose detail says green.
 */
export function matchesText(theme: Theme, text: string): boolean {
  const terms = normalise(text).split(' ').filter(Boolean);
  if (terms.length === 0) return true;

  const haystack = normalise(
    [theme.name, theme.detail, theme.id, CATEGORY_BY_ID.get(theme.category)?.name ?? ''].join(' '),
  );

  return terms.every((term) => haystack.includes(term));
}

export function filterThemes(themes: readonly Theme[], query: ThemeQuery): Theme[] {
  return themes.filter((theme) => {
    if (query.category && theme.category !== query.category) return false;
    if (query.text && !matchesText(theme, query.text)) return false;
    return true;
  });
}

/** The filtered set, grouped for display, with empty groups dropped. */
export function groupByCategory(
  themes: readonly Theme[],
): { category: CategoryId; themes: Theme[] }[] {
  const groups = new Map<CategoryId, Theme[]>();

  for (const theme of themes) {
    const existing = groups.get(theme.category);
    if (existing) existing.push(theme);
    else groups.set(theme.category, [theme]);
  }

  return [...groups.entries()].map(([category, list]) => ({ category, themes: list }));
}

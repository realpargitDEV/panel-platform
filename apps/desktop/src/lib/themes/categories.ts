/**
 * The eight groups, in the order the browser shows them.
 *
 * Ordered by how likely someone is to want the group, not alphabetically:
 * the person opening this screen for the first time is far more often looking
 * for a readable dark theme than for a Victorian pressure gauge.
 */

import type { CategoryId } from './types';

export interface Category {
  id: CategoryId;
  name: string;
  /** Shown under the heading, so a group explains itself before it is opened. */
  detail: string;
}

export const CATEGORIES: readonly Category[] = [
  {
    id: 'minimal',
    name: 'Minimal & Professional',
    detail: 'Quiet surfaces that stay out of the way. Safe for screen sharing.',
  },
  {
    id: 'developer',
    name: 'Developer',
    detail: 'The editor palettes, applied to the whole application.',
  },
  {
    id: 'hacker',
    name: 'Hacker & Cyber',
    detail: 'Terminal greens, warning reds and neon. Several animate.',
  },
  {
    id: 'futuristic',
    name: 'Futuristic',
    detail: 'Glass, glow and instrument panels.',
  },
  {
    id: 'gaming',
    name: 'Gaming',
    detail: 'Loud, high-contrast and unapologetic.',
  },
  {
    id: 'nature',
    name: 'Nature',
    detail: 'Landscapes and weather, at the saturation of a room rather than a photograph.',
  },
  {
    id: 'retro',
    name: 'Retro & Historical',
    detail: 'Interfaces from before this one. Square corners and system fonts.',
  },
  {
    id: 'creative',
    name: 'Creative & Unusual',
    detail: 'The ones that commit to an idea. Try them before a long session.',
  },
];

export const CATEGORY_BY_ID: ReadonlyMap<CategoryId, Category> = new Map(
  CATEGORIES.map((category) => [category.id, category]),
);

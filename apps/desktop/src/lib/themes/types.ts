/**
 * What a theme is allowed to be.
 *
 * The shape of this file is the whole safety argument for shipping eighty of
 * them. A theme may restate colour, type, shape and depth — the things that
 * make Windows 95 feel like Windows 95 rather than the same app in grey. It may
 * not touch spacing, layout, component structure or stacking order, because
 * those are the properties that decide whether a screen *works*, and eighty
 * chances to get one of them wrong is eighty screens someone has to open.
 *
 * Anything a theme leaves out is derived in `css.ts` rather than defaulted to a
 * fixed value, so a theme never inherits a colour from the charcoal default it
 * has nothing to do with.
 */

/** The eight groups the browser presents. Order here is display order. */
export type CategoryId =
  'hacker' | 'developer' | 'futuristic' | 'gaming' | 'minimal' | 'nature' | 'retro' | 'creative';

/**
 * The background effects a theme can ask for.
 *
 * Split by cost, which is the distinction that matters at runtime. The CSS
 * effects are gradients and repeating backgrounds: they cost a paint and
 * nothing else, so they are always safe to run. The canvas effects animate on
 * a frame loop, and every one of them is suspended when the window is hidden
 * or blurred and disabled outright when motion is reduced.
 */
export type CssEffectId = 'scanlines' | 'grid' | 'aurora' | 'pulse' | 'noise' | 'halftone';
export type CanvasEffectId =
  'rain' | 'stars' | 'particles' | 'blobs' | 'drizzle' | 'embers' | 'petals';
export type EffectId = CssEffectId | CanvasEffectId;

export const CSS_EFFECTS: readonly CssEffectId[] = [
  'scanlines',
  'grid',
  'aurora',
  'pulse',
  'noise',
  'halftone',
];

export const CANVAS_EFFECTS: readonly CanvasEffectId[] = [
  'rain',
  'stars',
  'particles',
  'blobs',
  'drizzle',
  'embers',
  'petals',
];

export function isCanvasEffect(effect: EffectId): effect is CanvasEffectId {
  return (CANVAS_EFFECTS as readonly string[]).includes(effect);
}

/**
 * The colours a theme states for itself.
 *
 * Only the ten required ones carry a theme's identity. The rest are optional
 * because most themes have no opinion about them: a theme that does not care
 * what shade of red an error is should not have to invent one, and one that
 * does — Red Team, Volcano — can say so.
 */
export interface ThemeTokens {
  /** Darkest surface: the page behind everything. */
  canvas: string;
  /** One step up: panels and cards. */
  surface: string;
  /** Two steps up: rows, inputs, the raised elements inside a card. */
  raised: string;
  /** Three steps up: menus and popovers, which float above everything. */
  overlay: string;
  /** Hairlines between elements on the same layer. */
  edge: string;
  /** Hairlines between regions, and the hover state of `edge`. */
  edgeStrong: string;

  /** Body text. Held to 4.5:1 against both canvas and raised by the gate. */
  ink: string;
  /** Secondary text. Also held to 4.5:1 — "muted" is not licence to be unreadable. */
  muted: string;
  /** Labels and disabled text, held to 3:1. Derived from `muted` when absent. */
  faint?: string;

  /** The one colour the theme is recognised by. */
  accent: string;

  /** Status colours, defaulted to the neutral set when the theme has no view. */
  ok?: string;
  warn?: string;
  danger?: string;

  /** The navigation rail's gradient. Derived from canvas and surface if absent. */
  sidebarTop?: string;
  sidebarBottom?: string;
}

/**
 * Everything about a theme that is not a colour.
 *
 * All optional: a theme that sets none of these gets the application's own
 * shapes and type, which is the right answer for most of the palette-led ones.
 */
export interface ThemeTraits {
  /** A system font stack. Never a downloaded face — the CSP blocks remote
   *  fonts, and an app that manages containers should not fetch one anyway. */
  fontUi?: string;
  fontMono?: string;

  /** Multiplier over the three radii. `0` squares every corner in the app. */
  radiusScale?: number;
  /** Border width for cards and controls, e.g. `'2px'` for Comic Book. */
  borderWidth?: string;

  shadowCard?: string;
  shadowRaised?: string;

  /** The colour of the glow around accented elements. Absent means no glow,
   *  so only the themes that ask for one pay for it. */
  glow?: string;

  /** One CSS background layered over the canvas — a gradient or a repeating
   *  pattern, never an image file. Blueprint's grid, Old Newspaper's grain. */
  texture?: string;

  /** Blur radius behind translucent surfaces, for the glass themes. */
  blur?: string;
}

export interface ThemeCredit {
  /** The name of the original work, which may differ from this theme's name. */
  work: string;
  author: string;
  licence: string;
  url: string;
}

export interface Theme {
  /** Stable, kebab-case, and written into `localStorage` — renaming one is a
   *  migration, not an edit. */
  id: string;
  name: string;
  category: CategoryId;
  /** One line, shown under the name in the browser. */
  detail: string;

  tokens: ThemeTokens;
  traits?: ThemeTraits;
  effect?: EffectId;

  /** Set where the palette is someone else's work. Rendered into
   *  THIRD-PARTY-THEMES.md and shown in the browser. */
  credit?: ThemeCredit;

  /** The contrast floor this theme is held to, in place of the default 4.5.
   *  High Contrast raises it; nothing lowers it. */
  contrastTarget?: number;
}

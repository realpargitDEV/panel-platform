/**
 * A theme, rendered to CSS.
 *
 * This is the only place that knows how a theme becomes a stylesheet, and it is
 * where every value a theme did not state gets derived. That division is the
 * point: eighty themes each state about a dozen colours, and the other thirty
 * tokens the application uses — hover states, tinted backgrounds, the editor's
 * whole palette, the sidebar gradient — are computed from them. A theme cannot
 * forget to restyle the editor, because it was never asked to.
 *
 * The brand gradient is the one derivation that exists for safety rather than
 * convenience. It carries white 13px text, so it is computed by darkening the
 * theme's accent until that text clears AA, rather than by trusting eighty
 * accents to each be dark enough.
 */

import {
  contrast,
  darken,
  darkenUntilWhiteReadable,
  isDark,
  mix,
  nudgeUntilReadable,
} from './color';
import type { Theme } from './types';

/** Status colours for the themes with no opinion about them. */
const NEUTRAL_OK = '#22c55e';
const NEUTRAL_WARN = '#eab308';
const NEUTRAL_DANGER = '#ef4444';

/** The three radii in `@theme`, which `radiusScale` multiplies. */
const BASE_RADII = { card: 12, control: 8, object: 10 } as const;

/** Every token the application reads, resolved for one theme. */
export interface ResolvedTheme {
  colour: Record<string, string>;
  trait: Record<string, string>;
}

/**
 * Fill in everything the theme left unsaid.
 *
 * Kept separate from the CSS text so it can be asserted on directly — the
 * contrast gate measures resolved values, not the abbreviated ones an author
 * wrote.
 */
export function resolveTheme(theme: Theme): ResolvedTheme {
  const t = theme.tokens;
  const dark = isDark(t.canvas);

  // `faint` is the one text weight a theme may skip: it is a step quieter than
  // muted, which is a relationship rather than a colour, so it can be derived
  // and still held to 3:1 by the same maths the gate uses.
  const faint = t.faint ?? nudgeUntilReadable(mix(t.muted, t.canvas, 0.35), t.canvas, 3);

  const ok = t.ok ?? NEUTRAL_OK;
  const warn = t.warn ?? NEUTRAL_WARN;
  const danger = t.danger ?? NEUTRAL_DANGER;

  // Tints sit behind text, so they stay close to the canvas: 16–18% of the
  // status colour is enough to read as "this row is selected/failed" without
  // dropping the contrast of the text on top of it.
  const softOf = (colour: string) => mix(t.canvas, colour, 0.16);

  const accentHover = dark ? mix(t.accent, '#000000', 0.1) : mix(t.accent, '#000000', 0.12);
  const accentSoft = mix(t.canvas, t.accent, 0.18);

  // White text sits on this gradient. Computed, never trusted.
  const brandFrom = darkenUntilWhiteReadable(t.accent, 4.5);
  const brandTo = darken(brandFrom, 0.22);
  const brandToHover = darken(brandFrom, 0.1);

  const sidebarTop = t.sidebarTop ?? mix(t.canvas, t.surface, 0.7);
  const sidebarBottom = t.sidebarBottom ?? mix(t.canvas, '#000000', 0.06);

  const colour: Record<string, string> = {
    canvas: t.canvas,
    surface: t.surface,
    raised: t.raised,
    overlay: t.overlay,
    edge: t.edge,
    'edge-strong': t.edgeStrong,

    ink: t.ink,
    muted: t.muted,
    faint,

    accent: t.accent,
    'accent-hover': accentHover,
    'accent-soft': accentSoft,

    ok,
    'ok-soft': softOf(ok),
    warn,
    'warn-soft': softOf(warn),
    danger,
    'danger-soft': softOf(danger),

    'sidebar-top': sidebarTop,
    'sidebar-bottom': sidebarBottom,
    'sidebar-edge': t.edge,

    'brand-from': brandFrom,
    'brand-to': brandTo,
    'brand-to-hover': brandToHover,

    // The editor. Derived rather than authored, because a theme that recoloured
    // the application and left the workspace charcoal would look broken in the
    // one screen people spend the longest in.
    'vs-titlebar': mix(t.canvas, t.surface, 0.4),
    'vs-activity': t.canvas,
    'vs-sidebar': mix(t.canvas, t.surface, 0.6),
    'vs-editor': t.surface,
    'vs-tabbar': mix(t.canvas, t.surface, 0.4),
    'vs-panel': mix(t.canvas, t.surface, 0.6),
    'vs-status': brandTo,
    'vs-status-idle': t.edgeStrong,
    'vs-border': t.edge,
    'vs-selected': mix(t.surface, t.accent, 0.18),
    'vs-active': mix(t.surface, t.accent, 0.28),
    'vs-text': t.ink,
    'vs-dim': t.muted,
    'vs-badge': t.accent,
  };

  const traits = theme.traits ?? {};
  const trait: Record<string, string> = {};

  if (traits.fontUi) trait['font-ui'] = traits.fontUi;
  if (traits.fontMono) trait['font-mono-theme'] = traits.fontMono;
  if (traits.borderWidth) trait['border-w'] = traits.borderWidth;
  if (traits.shadowCard) trait['shadow-card'] = traits.shadowCard;
  if (traits.shadowRaised) trait['shadow-raised'] = traits.shadowRaised;
  if (traits.glow) trait.glow = traits.glow;
  if (traits.texture) trait.texture = traits.texture;
  if (traits.blur) trait.blur = traits.blur;

  if (traits.radiusScale !== undefined) {
    const scale = traits.radiusScale;
    trait['radius-card'] = `${round(BASE_RADII.card * scale)}px`;
    trait['radius-control'] = `${round(BASE_RADII.control * scale)}px`;
    trait['radius-object'] = `${round(BASE_RADII.object * scale)}px`;
  }

  return { colour, trait };
}

function round(value: number): number {
  return Math.round(value * 100) / 100;
}

/**
 * The alias names, emitted beside the canonical ones.
 *
 * `--color-canvas` and friends are what Tailwind builds every utility class in
 * the application from, and they are not renamed. These are pointers at the
 * same values under the vocabulary a theme author would expect, so custom CSS —
 * and any user-written theme later — has names that describe roles rather than
 * this application's internal ramp.
 */
const ALIASES: readonly [string, string][] = [
  ['--background', '--color-canvas'],
  ['--surface', '--color-surface'],
  ['--surface-hover', '--color-overlay'],
  ['--primary', '--color-accent'],
  ['--secondary', '--color-edge-strong'],
  ['--text', '--color-ink'],
  ['--text-muted', '--color-muted'],
  ['--border', '--color-edge'],
  ['--success', '--color-ok'],
  ['--warning', '--color-warn'],
  ['--danger', '--color-danger'],
];

/** One `:root[data-theme='…']` block. */
export function renderTheme(theme: Theme): string {
  const { colour, trait } = resolveTheme(theme);
  const lines: string[] = [];

  lines.push(`/* ${theme.name} — ${theme.detail} */`);
  if (theme.credit) {
    lines.push(
      `/* Palette: ${theme.credit.work} by ${theme.credit.author} (${theme.credit.licence}) */`,
    );
  }
  lines.push(`:root[data-theme='${theme.id}'] {`);

  for (const [name, value] of Object.entries(colour)) {
    lines.push(`  --color-${name}: ${value};`);
  }
  for (const [name, value] of Object.entries(trait)) {
    lines.push(`  --${name}: ${value};`);
  }

  lines.push('');
  for (const [alias, canonical] of ALIASES) {
    lines.push(`  ${alias}: var(${canonical});`);
  }
  lines.push('  --border-radius: var(--radius-card);');
  lines.push('  --font-family: var(--font-ui);');
  lines.push('  --shadow: var(--shadow-card);');
  lines.push('}');

  // Square themes. Tailwind's radius utilities are literal values in the
  // markup, so re-pointing the radius tokens alone would leave most corners
  // rounded. Two attributes of specificity is enough to win against a single
  // utility class without reaching for `!important`.
  if (theme.traits?.radiusScale === 0) {
    lines.push(`:root[data-theme='${theme.id}'] [class*='rounded'] {`);
    lines.push('  border-radius: 0;');
    lines.push('}');
  }

  if (theme.traits?.borderWidth) {
    lines.push(`:root[data-theme='${theme.id}'] .border {`);
    lines.push('  border-width: var(--border-w);');
    lines.push('}');
  }

  // Glass. A blur behind an opaque panel is invisible, so the panels have to
  // become translucent at the same time — which is why this is one rule rather
  // than a `--blur` token the components opt into and mostly would not.
  if (theme.traits?.blur) {
    for (const [utility, token] of [
      ['bg-surface', '--color-surface'],
      ['bg-raised', '--color-raised'],
      ['bg-overlay', '--color-overlay'],
    ]) {
      lines.push(`:root[data-theme='${theme.id}'] .${utility} {`);
      lines.push(`  background-color: color-mix(in srgb, var(${token}) 78%, transparent);`);
      lines.push('  backdrop-filter: blur(var(--blur));');
      lines.push('}');
    }
  }

  // Glow. Confined to the two elements that are already the accent — the
  // primary action and the workspace mark — rather than applied to anything
  // accented, which at eighty themes would put a halo behind body text.
  if (theme.traits?.glow) {
    lines.push(`:root[data-theme='${theme.id}'] .btn-brand,`);
    lines.push(`:root[data-theme='${theme.id}'] .brand-tile {`);
    lines.push('  box-shadow:');
    lines.push('    0 1px 2px rgb(0 0 0 / 0.35),');
    lines.push('    0 0 20px color-mix(in srgb, var(--glow) 40%, transparent);');
    lines.push('}');
  }

  return lines.join('\n');
}

/** The whole catalogue as one stylesheet. */
export function renderAllThemes(themes: readonly Theme[]): string {
  const header = [
    '/*',
    ' * GENERATED FILE — do not edit.',
    ' *',
    ' * Every block here is rendered from the theme catalogue in',
    ' * `src/lib/themes/catalogue/`. Editing a colour here would be undone the',
    ' * next time it is regenerated, and the swatch shown in Settings would',
    ' * disagree with the theme.',
    ' *',
    ' * Regenerate:  pnpm test:themes:update',
    ' */',
    '',
  ].join('\n');

  return `${header}${themes.map(renderTheme).join('\n\n')}\n`;
}

/**
 * The contrast measurements the gate asserts on.
 *
 * Returned as data rather than asserted here so a failing theme can be reported
 * with its numbers — "muted 3.9:1 on canvas" tells an author what to change,
 * where "theme failed" does not.
 */
export interface ThemeContrast {
  inkOnCanvas: number;
  inkOnSurface: number;
  inkOnRaised: number;
  mutedOnCanvas: number;
  mutedOnSurface: number;
  faintOnCanvas: number;
  accentOnCanvas: number;
  accentOnSurface: number;
  whiteOnBrand: number;
}

export function measureTheme(theme: Theme): ThemeContrast {
  const { colour } = resolveTheme(theme);

  // Every one of these is written by `resolveTheme` a few lines above, so a
  // missing key is a bug in this file rather than a theme that omitted
  // something. Throwing names it; defaulting to black would silently pass the
  // gate with a measurement nobody took.
  const need = (key: string): string => {
    const value = colour[key];
    if (value === undefined) throw new Error(`resolveTheme produced no --color-${key}`);
    return value;
  };

  return {
    inkOnCanvas: contrast(need('ink'), need('canvas')),
    inkOnSurface: contrast(need('ink'), need('surface')),
    inkOnRaised: contrast(need('ink'), need('raised')),
    mutedOnCanvas: contrast(need('muted'), need('canvas')),
    mutedOnSurface: contrast(need('muted'), need('surface')),
    faintOnCanvas: contrast(need('faint'), need('canvas')),
    accentOnCanvas: contrast(need('accent'), need('canvas')),
    accentOnSurface: contrast(need('accent'), need('surface')),
    whiteOnBrand: contrast('#ffffff', need('brand-from')),
  };
}

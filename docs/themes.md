# Themes

Eighty-one themes in eight categories, built from one engine. This is how it
works, what a theme may and may not change, and what to do to add one.

## The shape of it

```
src/lib/themes/
  types.ts        what a theme is allowed to be
  color.ts        hex, WCAG contrast, mixing — no framework, no DOM
  css.ts          a theme → a :root[data-theme] block, and the derivations
  categories.ts   the eight groups, in display order
  catalogue/      the palettes, ten to a file
  effects.ts      the canvas painters (no React, no timers)
  search.ts       the browser's filtering, as pure functions
  index.ts        the flat list, lookups and id migration
src/themes.generated.css     generated; imported by styles.css
src/components/ThemeBrowser.tsx   choosing one
src/components/ThemeEffects.tsx   the background layer and its frame loop
```

A theme is **data**, not a stylesheet. `applyAppearance` writes
`data-theme="matrix-rain"` onto the root element and the matching block in the
generated stylesheet takes over. Switching themes is one attribute write: no
component re-renders, and nothing in the tree knows which theme is on.

## What a theme may change

**Colour** — four surface layers (`canvas`, `surface`, `raised`, `overlay`), two
border weights, three text weights (`ink`, `muted`, `faint`), one `accent`, and
optionally the three status colours and the sidebar gradient.

**Type** — `fontUi` and `fontMono`. System stacks only. The application ships no
font files and the packaged build's CSP blocks remote ones.

**Shape** — `radiusScale`, a multiplier over the three radii. `0` squares every
corner in the application.

**Depth** — `borderWidth`, `shadowCard`, `shadowRaised`, `glow`, `blur`.

**Texture** — one CSS background over the canvas. A gradient or a repeating
pattern, never an image file.

## What a theme may never change

Spacing, layout, component structure, stacking order. That line is the reason
eighty-one themes are safe: none of them can move an element, so no screen has
to be checked eighty-one times.

## What is derived rather than authored

A theme states about a dozen colours. `resolveTheme` produces about forty. The
rest are computed:

- **hover and `-soft` tints** — mixed from the accent and the canvas.
- **the sidebar gradient** — from canvas and surface, unless stated.
- **the whole editor palette** (`--color-vs-*`) — from canvas, surface and
  accent. A theme cannot forget to restyle the workspace, because it was never
  asked to.
- **`faint`** — from `muted` toward the canvas, held at 3:1.
- **the brand gradient** — the accent, darkened step by step until white 13px
  text on it clears 4.5:1. This one exists for safety rather than convenience:
  the primary action carries white text in every theme, and eighty-one accents
  cannot each be trusted to be dark enough. `#3b82f6` measures 3.68:1 and fails.

## The contrast gate

`themes.test.ts` measures every theme and fails the build on any that cannot be
read:

| Measurement                         | Floor |
| ----------------------------------- | ----- |
| `ink` on canvas, surface and raised | 4.5:1 |
| `muted` on canvas and surface       | 4.5:1 |
| `faint` on canvas                   | 3:1   |
| `accent` on canvas and surface      | 3:1   |
| white on the brand gradient         | 4.5:1 |

High Contrast raises its own floor to 7:1 via `contrastTarget`. Nothing lowers
it. Failures name the theme and quote the number, so the fix is obvious.

## Effects

A theme may declare one background effect. There are two kinds and only one
costs anything.

**CSS effects** — `scanlines`, `grid`, `aurora`, `pulse`, `noise`, `halftone`.
A `data-effect` attribute and a stylesheet rule. They paint and stop.

**Canvas effects** — `rain`, `stars`, `particles`, `blobs`, `drizzle`, `embers`,
`petals`. These animate, and are governed:

- suspended when the window is hidden, and when it loses focus;
- not rendered at all when Motion is `reduced` or `off`, or when the operating
  system asks for reduced motion;
- frame delta clamped to 50ms, so a throttled window does not teleport every
  particle across the screen on its first frame back;
- particle counts scale per megapixel and are capped, so a 4K window costs a
  predictable maximum rather than four times a 1080p one;
- device pixel ratio capped at 2.

The painters live in `effects.ts` as plain objects with `resize` and `frame`.
They hold no timer and know nothing about React, which is why they are tested
without a browser: counts, bounds after ten seconds of simulated frames, and
behaviour on a zero-length or half-second frame.

## Adding a theme

1. Add it to the right file in `catalogue/`. Ten per category is the
   convention, not a rule the code enforces — but `themes.test.ts` asserts the
   count, so update that test deliberately if the number changes.
2. Run `pnpm test:themes:update` to regenerate `themes.generated.css`.
3. Run `pnpm test`. The contrast gate will tell you if the palette is not
   readable, and which value to move.

Never edit `themes.generated.css`. It is regenerated from the catalogue and a
test fails if the two disagree.

## Accent

Each theme ships the accent it was designed with, and `Auto` — the default —
uses it. The six named accents still override any theme, for anyone who wants
that; choosing one writes `data-accent`, which beats the theme's own accent on
source order. `Auto` removes the attribute rather than writing a value, so a
previous choice cannot go on quietly winning.

## Migration

Five theme ids existed before the catalogue: `dark`, `light`, `amber`,
`midnight`, `nord`. `dark` and `light` became `pure-dark` and `pure-light`; the
other three kept their ids. `LEGACY_THEME_IDS` in `index.ts` is the map, and it
is the only reason an existing installation does not silently reset to the
default on upgrade. Entries may be added to it; none may be removed.

## Third-party palettes

Eight themes are other people's colour schemes, credited in
[THIRD-PARTY-THEMES.md](../THIRD-PARTY-THEMES.md). Seven more were requested
under trademarked names and ship under descriptive ones.

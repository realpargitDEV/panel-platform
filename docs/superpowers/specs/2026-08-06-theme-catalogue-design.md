# Theme catalogue and effects engine

**Date:** 2026-08-06
**Status:** Implemented

## The ask

Grow the theme system from five themes to eighty across ten categories, add a
category browser so themes can be found rather than scrolled past, and build one
token-driven engine rather than eighty stylesheets — with advanced themes able
to opt into effects: code rain, scan lines, animated gradients, particles, glow,
background blur.

## What the count actually was

The requested list contains **eighty themes in eight categories**, not ten:
Hacker & Cyber, Developer, Futuristic, Gaming, Minimal & Professional, Nature,
Retro & Historical, Creative & Unusual — ten each. Shipped as **eighty-one**:
`Amber` is kept because it was a real setting someone may have chosen, and
deleting a theme is not the same as renaming one.

## Decisions

| Decision        | Chosen                                            | Why                                                                                                                            |
| --------------- | ------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------ |
| Delivery        | Phased: engine → palettes → effects               | A wrong call in the token contract means redoing many themes; phase 1 surfaces that early.                                     |
| Theme reach     | Colour + type + shape + depth                     | Windows 95 and Comic Book fail as recolours. Layout and structure stay fixed so no theme can break a screen.                   |
| Accent          | Theme owns it; `Auto` default, six overrides kept | 486 theme×accent combinations nobody has looked at, versus each theme arriving as designed.                                    |
| Effects         | One background layer, suspended when unwatched    | The app sits open all day; a canvas burning CPU behind a minimised window is how a theme gets blamed for a laptop fan.         |
| Readability     | Automated WCAG gate over all 81                   | Eighty-one palettes is eighty-one chances to ship unreadable text and no reviewer opens them all.                              |
| Source of truth | TypeScript catalogue → generated CSS              | The browser UI, the gate and the stylesheet read one definition, so a swatch cannot advertise a colour the theme does not use. |
| Borrowed names  | Trademarks renamed; MIT schemes credited          | The same standard already applied to the Discord glyph in this repo.                                                           |

## Token contract

Existing `--color-*` names are kept — Tailwind v4 generates every utility class
in the application from them, so renaming would be whole-codebase churn for no
gain. Added: `--font-ui`, `--font-mono-theme`, `--radius-*` (scaled),
`--border-w`, `--shadow-card`, `--shadow-raised`, `--glow`, `--texture`,
`--blur`.

Each generated block also emits the requested vocabulary — `--background`,
`--surface`, `--surface-hover`, `--primary`, `--secondary`, `--text`,
`--text-muted`, `--border`, `--success`, `--warning`, `--danger`, `--shadow`,
`--glow`, `--border-radius`, `--font-family` — as pointers to the canonical
tokens, giving custom CSS and any future user-authored theme role-named
variables without renaming anything internally.

A theme states about a dozen colours; `resolveTheme` derives about forty,
including the entire editor palette and a brand gradient computed by darkening
the accent until white 13px text clears AA.

## Architecture

See [docs/themes.md](../../themes.md) for the working reference. In short:
`catalogue/` (8 files × ~10 palettes) → `css.ts` → `themes.generated.css`,
imported by `styles.css`; `ThemeBrowser` for selection; `ThemeEffects` +
`effects.ts` for backgrounds; `appearance.ts` for state, validation and
migration of the five original ids.

## Verification

- 1298 TypeScript tests, 975 Rust.
- Contrast gate: all 81 themes pass; High Contrast held to 7:1.
- Production build green; all 81 blocks confirmed present in the compiled CSS.
- Effect painters tested headless: counts, caps, bounds after 600 simulated
  frames, zero-length and half-second frames.

## Seen running

The frontend was driven in a browser against the Vite dev server (the Tauri
backend is absent, so the core calls fail and the shell reports it — the styling
is unaffected). Confirmed by looking:

- The browser renders: 81 counted, eight category chips, results grouped under
  their headings. Searching `falling code` returns Matrix Rain alone, on
  description text; `halftone` returns two themes across two categories.
- **Matrix Rain** applies across the whole shell, and the code rain is visible
  behind the interface with cards and the rail correctly on top. This was the
  main layering risk — a fixed layer at `z-index: 0` under a shell lifted to 1 —
  and it holds.
- **System 95** squares every corner, including the accent swatches that are
  otherwise circles, and picks up the legacy face. The `[class*='rounded']`
  override is what makes that reach Tailwind's literal radius utilities.
- **Comic Book** draws three-pixel outlines on every element, hard offset
  shadows, the rounded face and a visible halftone screen.
- **Glass Minimal** resolves `--blur: 18px` and `backdrop-filter: blur(18px)` on
  all three surface utilities.
- Accent `Auto` writes no `data-accent`, and its swatch takes the theme's own
  accent — green under Matrix Rain, navy under System 95.
- The miniatures keep their own palettes while a different theme is applied,
  which is the property that stops a card advertising a colour it does not use.

## Not verified

**The canvas effects have not been seen animating.** Chrome reports
`document.hidden === true` for an automated window and fires zero animation
frames, so the loop cannot run there — which is the suspend behaviour working,
not a fault. The rain was instead confirmed by stepping the painter manually
into the real canvas with the theme's live token values: 120 columns, ~7,000 lit
pixels after 120 frames, and the result is visible in the screenshot. What
remains unproven is the loop's _own_ behaviour in a watched window: that it
starts, holds a steady frame rate, and suspends on blur.

Seventy-seven of the eighty-one themes have not been looked at individually.
They are held by measurement, not by eye.

## Deliberately not built

- User-authored themes. The engine would support it; the UI, storage and
  validation for it are a separate piece of work.
- Per-component effects. One background layer only, so cost stays predictable.
- More than one effect at a time.

/**
 * The type a theme can ask for.
 *
 * System stacks only. The application ships no font files and the packaged
 * build's CSP blocks remote ones, so a stack that names a face the machine does
 * not have must degrade to something sensible rather than to whatever the
 * browser picks — which is why every stack here ends in a generic family.
 *
 * These are what makes DOS read as DOS and Old Newspaper read as newsprint. A
 * theme that says nothing about type keeps the application's own.
 */

export const MONO =
  "ui-monospace, 'Cascadia Mono', 'JetBrains Mono', 'SF Mono', Menlo, Consolas, monospace";

/** The blockier terminal end of monospace, for the DOS and terminal themes. */
export const TERMINAL = "'Consolas', 'Lucida Console', 'Courier New', monospace";

export const SYSTEM =
  "system-ui, -apple-system, 'Segoe UI', Roboto, 'Helvetica Neue', Arial, sans-serif";

/** The 1995 Windows stack. Tahoma is the closest thing still installed. */
export const SYSTEM_LEGACY = "Tahoma, 'MS Sans Serif', 'Segoe UI', Geneva, sans-serif";

/** Old Macintosh chrome: Geneva and Chicago are long gone, Verdana is the
 *  nearest survivor with the same wide, even fit. */
export const SYSTEM_CLASSIC = "Geneva, Verdana, 'Segoe UI', Tahoma, sans-serif";

export const SERIF = "'Iowan Old Style', 'Palatino Linotype', Palatino, Georgia, serif";

/** Newsprint: a narrow, high-contrast serif with a headline feel. */
export const SERIF_NEWS = "'Times New Roman', Times, 'Liberation Serif', Georgia, serif";

/** Display serif for the ornate themes — Victorian, Luxury, Steampunk. */
export const SERIF_DISPLAY = "'Baskerville', 'Bodoni MT', 'Didot', Georgia, serif";

/** Rounded and friendly, for the comic and anime themes. */
export const ROUNDED = "'Comic Sans MS', 'Chalkboard SE', 'Segoe UI', system-ui, sans-serif";

/** Squared-off and technical, for HUDs and instrument panels. */
export const TECHNICAL = "'Bahnschrift', 'DIN Alternate', 'Segoe UI', system-ui, sans-serif";

/** Wide geometric, for the arcade and gaming themes. */
export const DISPLAY_WIDE = "'Impact', 'Haettenschweiler', 'Arial Black', system-ui, sans-serif";

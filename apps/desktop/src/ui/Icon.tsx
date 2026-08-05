/**
 * The application's icons.
 *
 * Plain SVG rather than an icon font: a font would be a network request the CSP
 * forbids, or a binary bundled into every install to draw two dozen glyphs. All
 * of them are 16×16 stroke drawings in `currentColor`, so an icon takes the
 * colour of whatever contains it and a hover state needs no second asset.
 *
 * # The rules that make them look like one set
 *
 * A set drawn without constraints is what makes an interface look assembled
 * rather than designed, so these are fixed and every glyph obeys them:
 *
 * - **One live area.** Everything is drawn inside 2–14, leaving a 2px margin.
 *   A glyph that used the full box would read as larger than its neighbours at
 *   the same nominal size.
 * - **One stroke: 1.5.** Matching `ProjectMark` and `Logo`, so an icon beside a
 *   project mark reads as the same hand. The set was previously 1.3, which went
 *   thin and grey next to everything else.
 * - **Round caps and joins**, set once on the `svg` rather than per path.
 * - **Radii of 1.5–2** on rounded rectangles, and circles on whole pixels
 *   where possible, so nothing lands on a half-pixel and blurs at 16px.
 * - **Fill is reserved for transport.** `play` and `stop` are solid because
 *   they are buttons whose state matters at a glance; everything else is a
 *   stroke drawing. Mixing the two arbitrarily is what makes a set look
 *   collected from several sources.
 *
 * The workspace has its own smaller set tuned for 15px rows; this one is for
 * the application shell and its pages.
 */
import type { ReactNode } from 'react';

export type IconName =
  | 'overview'
  | 'projects'
  | 'activity'
  | 'discord'
  | 'settings'
  | 'plus'
  | 'search'
  | 'chevron-down'
  | 'chevron-right'
  | 'chevron-left'
  | 'close'
  | 'alert'
  | 'info'
  | 'check'
  | 'check-circle'
  | 'play'
  | 'stop'
  | 'restart'
  | 'power'
  | 'refresh'
  | 'external'
  | 'folder'
  | 'file'
  | 'git'
  | 'download'
  | 'upload'
  | 'trash'
  | 'more'
  | 'grid'
  | 'list'
  | 'filter'
  | 'cpu'
  | 'memory'
  | 'disk'
  | 'clock'
  | 'network'
  | 'terminal'
  | 'logs'
  | 'shield'
  | 'bell'
  | 'user'
  | 'sidebar'
  | 'copy'
  | 'edit'
  | 'container'
  | 'arrow-right'
  | 'command'
  | 'blocked';

const PATHS: Record<IconName, ReactNode> = {
  // ---------------------------------------------------------- navigation
  /** A dashboard: one tall pane and two short ones, the layout it names. */
  overview: (
    <>
      <rect x="2.2" y="2.2" width="5" height="11.6" rx="1.5" />
      <rect x="8.8" y="2.2" width="5" height="5" rx="1.5" />
      <rect x="8.8" y="8.8" width="5" height="5" rx="1.5" />
    </>
  ),
  /** Stacked layers — several things kept, not one thing open. */
  projects: (
    <>
      <path d="M8 2.2l5.8 3-5.8 3-5.8-3z" />
      <path d="M2.2 8.4l5.8 3 5.8-3M2.2 11.4l5.8 3 5.8-3" />
    </>
  ),
  /** A pulse. Asymmetric on purpose: a symmetric zigzag reads as a chart. */
  activity: <path d="M2.2 8.4h2.6l1.9-4.6 2.6 8 1.7-3.4h2.8" />,
  /** A chat bubble with two eyes — an original mark rather than Discord's
   *  trademarked one, which this set has no licence to redraw. */
  discord: (
    <>
      <path d="M3.6 2.8h8.8a1.6 1.6 0 011.6 1.6v5.2a1.6 1.6 0 01-1.6 1.6H7.6l-3.4 2.4v-2.4h-.6A1.6 1.6 0 012 9.6V4.4a1.6 1.6 0 011.6-1.6z" />
      <path d="M6.2 6.8v.9M9.8 6.8v.9" />
    </>
  ),
  /** A gear with six teeth. Six rather than eight: at 16px, eight teeth merge
   *  into a circle. */
  settings: (
    <>
      <circle cx="8" cy="8" r="2.3" />
      <path d="M8 1.9v1.5M8 12.6v1.5M13.3 5l-1.3.75M4 10.25L2.7 11M13.3 11l-1.3-.75M4 5.75L2.7 5" />
    </>
  ),
  plus: <path d="M8 3.2v9.6M3.2 8h9.6" />,
  /** The handle leaves the circle on the 45°, which is what keeps it reading
   *  as a lens rather than a balloon. */
  search: (
    <>
      <circle cx="7.2" cy="7.2" r="4.2" />
      <path d="M10.4 10.4l3 3" />
    </>
  ),

  // -------------------------------------------------------------- chevrons
  // One length and one angle across all four, so a row of disclosure arrows
  // does not appear to change size with direction.
  'chevron-down': <path d="M4.2 6.4L8 10.2l3.8-3.8" />,
  'chevron-right': <path d="M6.4 4.2L10.2 8l-3.8 3.8" />,
  'chevron-left': <path d="M9.6 4.2L5.8 8l3.8 3.8" />,
  'arrow-right': <path d="M2.8 8h10M9 4.2L12.8 8 9 11.8" />,
  close: <path d="M4.2 4.2l7.6 7.6M11.8 4.2l-7.6 7.6" />,

  // ---------------------------------------------------------------- status
  alert: (
    <>
      <path d="M8 2.6l6 10.8H2z" />
      <path d="M8 6.6v3.1M8 11.6v.1" />
    </>
  ),
  info: (
    <>
      <circle cx="8" cy="8" r="5.8" />
      <path d="M8 7.4v3.4M8 5.2v.1" />
    </>
  ),
  check: <path d="M3.4 8.4l3.1 3.2 6.1-7.2" />,
  'check-circle': (
    <>
      <circle cx="8" cy="8" r="5.8" />
      <path d="M5.4 8.2l1.9 1.9 3.5-4.1" />
    </>
  ),
  blocked: (
    <>
      <circle cx="8" cy="8" r="5.8" />
      <path d="M4.1 11.9L11.9 4.1" />
    </>
  ),

  // ------------------------------------------------------------- transport
  // Solid, because these are the two controls whose state must read instantly.
  play: <path d="M5.2 3.4l7 4.6-7 4.6z" fill="currentColor" stroke="none" />,
  stop: (
    <rect x="4.2" y="4.2" width="7.6" height="7.6" rx="1.6" fill="currentColor" stroke="none" />
  ),
  /** A closed loop with one arrowhead: restarting returns to where it began. */
  restart: (
    <>
      <path d="M13.2 8a5.2 5.2 0 11-1.9-4" />
      <path d="M13.4 2.6v3.6h-3.6" />
    </>
  ),
  power: (
    <>
      <path d="M8 2.4v6" />
      <path d="M11.6 4.6a5 5 0 11-7.2 0" />
    </>
  ),
  /** Two arcs, each with its own head, so it reads as a cycle rather than as
   *  one arrow bent round. */
  refresh: (
    <>
      <path d="M13.1 7.2A5.2 5.2 0 004 4.6M2.9 8.8a5.2 5.2 0 009.1 2.6" />
      <path d="M13.4 3.6v3.6h-3.6M2.6 12.4V8.8h3.6" />
    </>
  ),
  external: (
    <>
      <path d="M12.6 9.4v3a1.4 1.4 0 01-1.4 1.4H3.6a1.4 1.4 0 01-1.4-1.4V4.8a1.4 1.4 0 011.4-1.4h3" />
      <path d="M9.4 2.4h4.2v4.2M13.6 2.4L7.8 8.2" />
    </>
  ),

  // ------------------------------------------------------------------ files
  /** The tab sits on the left and the body is one shape, so it does not read
   *  as two rectangles at small sizes. */
  folder: (
    <>
      <path d="M2.2 12.4V4.2a1.2 1.2 0 011.2-1.2h2.6l1.6 1.8h5.2a1.2 1.2 0 011.2 1.2v6.4a1.2 1.2 0 01-1.2 1.2H3.4a1.2 1.2 0 01-1.2-1.2z" />
    </>
  ),
  file: (
    <>
      <path d="M9 2.2H4.6a1.4 1.4 0 00-1.4 1.4v8.8a1.4 1.4 0 001.4 1.4h6.8a1.4 1.4 0 001.4-1.4V5.6z" />
      <path d="M9 2.2v3.4h3.8" />
    </>
  ),
  logs: (
    <>
      <path d="M9 2.2H4.6a1.4 1.4 0 00-1.4 1.4v8.8a1.4 1.4 0 001.4 1.4h6.8a1.4 1.4 0 001.4-1.4V5.6z" />
      <path d="M9 2.2v3.4h3.8M5.6 8.6h4.8M5.6 11h3.2" />
    </>
  ),
  copy: (
    <>
      <rect x="5.6" y="5.6" width="8.2" height="8.2" rx="1.6" />
      <path d="M10.4 5.6V3.6a1.4 1.4 0 00-1.4-1.4H3.6a1.4 1.4 0 00-1.4 1.4v5.4a1.4 1.4 0 001.4 1.4h2" />
    </>
  ),
  /** A pencil with a nib. The nib is the difference between a pencil and a
   *  rectangle on a diagonal. */
  edit: (
    <>
      <path d="M11.1 2.6l2.3 2.3-7.7 7.7-3 .7.7-3z" />
      <path d="M9.7 4l2.3 2.3" />
    </>
  ),
  trash: (
    <>
      <path d="M3 4.6h10" />
      <path d="M6.2 4.6V3.4a1 1 0 011-1h1.6a1 1 0 011 1v1.2" />
      <path d="M4.4 4.6l.7 8.2a1.2 1.2 0 001.2 1.1h3.4a1.2 1.2 0 001.2-1.1l.7-8.2" />
    </>
  ),
  download: (
    <>
      <path d="M8 2.4v7.2M5 6.8L8 9.8l3-3" />
      <path d="M2.6 11.4v1a1.4 1.4 0 001.4 1.4h8a1.4 1.4 0 001.4-1.4v-1" />
    </>
  ),
  upload: (
    <>
      <path d="M8 9.8V2.6M5 5.6L8 2.6l3 3" />
      <path d="M2.6 11.4v1a1.4 1.4 0 001.4 1.4h8a1.4 1.4 0 001.4-1.4v-1" />
    </>
  ),
  /** A branch: two commits and the fork between them. */
  git: (
    <>
      <circle cx="4.6" cy="3.8" r="1.7" />
      <circle cx="4.6" cy="12.2" r="1.7" />
      <circle cx="11.4" cy="7.4" r="1.7" />
      <path d="M4.6 5.5v5M9.9 8.6c-1 1.5-2.6 1.9-4 1.9" />
    </>
  ),

  // ------------------------------------------------------------ views, menus
  more: (
    <>
      <circle cx="3.6" cy="8" r="1.05" fill="currentColor" stroke="none" />
      <circle cx="8" cy="8" r="1.05" fill="currentColor" stroke="none" />
      <circle cx="12.4" cy="8" r="1.05" fill="currentColor" stroke="none" />
    </>
  ),
  grid: (
    <>
      <rect x="2.4" y="2.4" width="5" height="5" rx="1.4" />
      <rect x="8.6" y="2.4" width="5" height="5" rx="1.4" />
      <rect x="2.4" y="8.6" width="5" height="5" rx="1.4" />
      <rect x="8.6" y="8.6" width="5" height="5" rx="1.4" />
    </>
  ),
  /** Bullets and rules, which distinguishes a list from a paragraph. */
  list: (
    <>
      <path d="M6 4h7.6M6 8h7.6M6 12h7.6" />
      <circle cx="3.2" cy="4" r="0.85" fill="currentColor" stroke="none" />
      <circle cx="3.2" cy="8" r="0.85" fill="currentColor" stroke="none" />
      <circle cx="3.2" cy="12" r="0.85" fill="currentColor" stroke="none" />
    </>
  ),
  filter: <path d="M2.4 3.4h11.2l-4.3 5v4.6l-2.6 1.2V8.4z" />,
  sidebar: (
    <>
      <rect x="2.2" y="2.8" width="11.6" height="10.4" rx="1.6" />
      <path d="M6.6 2.8v10.4" />
    </>
  ),
  command: (
    <>
      <path d="M6 4.5a1.5 1.5 0 10-1.5 1.5H6zm0 0h4m0 0V4.5A1.5 1.5 0 1111.5 6H10zm0 0v4m0 0h1.5a1.5 1.5 0 11-1.5 1.5V10zm0 0H6m0 0v1.5A1.5 1.5 0 114.5 10H6z" />
    </>
  ),

  // -------------------------------------------------------------- resources
  /** A chip with pins on all four sides. */
  cpu: (
    <>
      <rect x="4.4" y="4.4" width="7.2" height="7.2" rx="1.4" />
      <rect x="6.6" y="6.6" width="2.8" height="2.8" rx="0.7" />
      <path d="M6.4 2.2v2.2M9.6 2.2v2.2M6.4 11.6v2.2M9.6 11.6v2.2M2.2 6.4h2.2M2.2 9.6h2.2M11.6 6.4h2.2M11.6 9.6h2.2" />
    </>
  ),
  /** A module with contacts along its base. */
  memory: (
    <>
      <rect x="2.2" y="4.2" width="11.6" height="6.4" rx="1.3" />
      <path d="M5 10.6v1.6M8 10.6v1.6M11 10.6v1.6M5.6 6.6v1.8M8 6.6v1.8M10.4 6.6v1.8" />
    </>
  ),
  /** A platter stack, seen from the side. */
  disk: (
    <>
      <ellipse cx="8" cy="4.4" rx="5.6" ry="2.2" />
      <path d="M2.4 4.4v7.2c0 1.2 2.5 2.2 5.6 2.2s5.6-1 5.6-2.2V4.4" />
      <path d="M13.6 8c0 1.2-2.5 2.2-5.6 2.2S2.4 9.2 2.4 8" />
    </>
  ),
  clock: (
    <>
      <circle cx="8" cy="8" r="5.8" />
      <path d="M8 4.8V8l2.3 1.6" />
    </>
  ),
  /** Three nodes and the links between them. */
  network: (
    <>
      <circle cx="8" cy="3.4" r="1.7" />
      <circle cx="3.4" cy="12.4" r="1.7" />
      <circle cx="12.6" cy="12.4" r="1.7" />
      <path d="M6.9 4.9l-2.6 5.9M9.1 4.9l2.6 5.9M5.1 12.4h5.8" />
    </>
  ),
  terminal: (
    <>
      <rect x="2.2" y="2.8" width="11.6" height="10.4" rx="1.6" />
      <path d="M5 6.6l1.9 1.7L5 10M8.8 10.4h2.6" />
    </>
  ),
  container: (
    <>
      <path d="M8 2.2l5.6 2.8v6L8 13.8 2.4 11V5z" />
      <path d="M2.4 5l5.6 2.8L13.6 5M8 7.8v6" />
    </>
  ),

  // --------------------------------------------------------------- identity
  shield: (
    <>
      <path d="M8 2.2l5 1.9v4c0 3-2.1 5-5 5.7-2.9-.7-5-2.7-5-5.7v-4z" />
    </>
  ),
  bell: (
    <>
      <path d="M4.4 11.2V7.4a3.6 3.6 0 017.2 0v3.8" />
      <path d="M3 11.2h10M6.6 13.2a1.5 1.5 0 002.8 0" />
    </>
  ),
  user: (
    <>
      <circle cx="8" cy="5.6" r="2.7" />
      <path d="M3 13.6a5 5 0 0110 0" />
    </>
  ),
};

export default function Icon({
  name,
  size = 16,
  className = '',
}: {
  name: IconName;
  size?: number;
  className?: string;
}) {
  return (
    <svg
      aria-hidden
      focusable="false"
      width={size}
      height={size}
      viewBox="0 0 16 16"
      fill="none"
      stroke="currentColor"
      // 1.5 rather than 1.3: it matches `ProjectMark` and `Logo`, so an icon
      // next to a project mark reads as the same hand rather than a lighter one.
      strokeWidth="1.5"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={`shrink-0 ${className}`}
    >
      {PATHS[name]}
    </svg>
  );
}

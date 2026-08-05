/**
 * What a project looks like at a glance.
 *
 * Replaces the first letter of the display name in a coloured square, which
 * told the reader nothing they could not already read two centimetres to the
 * right.
 *
 * Two signals, deliberately separated:
 *   - the **glyph** says what the project is — its runtime — and is the same
 *     for every Node project, so the shape becomes learnable;
 *   - the **colour** says which project it is, derived from its id, so the same
 *     project is recognisable in a list, in the header and in the switcher.
 *
 * The glyphs are drawn here rather than taken from the vendors. They are
 * house-style abstractions — a hexagon for Node, a gem for Ruby — because a
 * folder of pasted brand PNGs is both a licensing question and the exact
 * "assembled from GitHub" look this set exists to avoid. They share one grid,
 * one stroke weight and one optical size, which is what makes a row of them
 * look designed rather than collected.
 */
import { identityFor } from '../lib/identity';

/** Every glyph is drawn on the same 16×16 grid with the same 1.5 stroke, so
 *  they sit at one optical weight beside each other. */
const GLYPHS: Record<string, React.ReactNode> = {
  // A hexagon: Node's package shape, reduced to its outline.
  NODEJS: <path d="M8 1.8l5.2 3v6.4L8 14.2l-5.2-3V4.8z" />,
  // The hexagon with a bar through it — a compile step over the same runtime.
  TYPESCRIPT: (
    <>
      <path d="M8 1.8l5.2 3v6.4L8 14.2l-5.2-3V4.8z" />
      <path d="M5.6 6.6h4.8M8 6.6v4.2" />
    </>
  ),
  // Two stacked arcs: the interpreter that comes in two halves.
  PYTHON: (
    <>
      <path d="M4.4 7.4V4.6a2 2 0 012-2h3.2a2 2 0 012 2v1.2a2 2 0 01-2 2H6.4a2 2 0 00-2 2v1.6" />
      <path d="M11.6 8.6v2.8a2 2 0 01-2 2H6.4a2 2 0 01-2-2" />
    </>
  ),
  // A circle cut by two rules — motion, held level.
  GO: (
    <>
      <circle cx="8" cy="8" r="5.6" />
      <path d="M2.6 6.6h4.2M2.6 9.4h4.2" />
    </>
  ),
  // A cog: the systems language, and the only glyph with radial teeth.
  RUST: (
    <>
      <circle cx="8" cy="8" r="3.2" />
      <path d="M8 1.6v1.6M8 12.8v1.6M14.4 8h-1.6M3.2 8H1.6M12.5 3.5l-1.1 1.1M4.6 11.4l-1.1 1.1M12.5 12.5l-1.1-1.1M4.6 4.6L3.5 3.5" />
    </>
  ),
  // A cup with rising steam.
  JAVA: (
    <>
      <path d="M3.6 7.4h7.2v3a2.4 2.4 0 01-2.4 2.4H6a2.4 2.4 0 01-2.4-2.4z" />
      <path d="M10.8 8.4h1a1.4 1.4 0 010 2.8h-1" />
      <path d="M6.2 5.2c0-1 1.2-1.2 1.2-2.2M8.8 5.2c0-.8.8-1 .8-1.8" />
    </>
  ),
  // A wide ellipse: PHP's elongated mark, abstracted.
  PHP: (
    <>
      <ellipse cx="8" cy="8" rx="6.2" ry="3.8" />
      <path d="M6 9.6l.9-3.2M9.1 9.6l.9-3.2" />
    </>
  ),
  // A cut gem.
  RUBY: (
    <>
      <path d="M4.2 3.2h7.6l2.4 3.4L8 13.4 1.8 6.6z" />
      <path d="M1.8 6.6h12.4M8 3.2v10.2" />
    </>
  ),
  // A bun: a dome on a base.
  BUN: (
    <>
      <path d="M2.4 9.6a5.6 4.4 0 0111.2 0z" />
      <path d="M2 11.6h12" />
    </>
  ),
  // A single confident arc — Deno's one-runtime idea.
  DENO: (
    <>
      <circle cx="8" cy="8" r="5.8" />
      <path d="M6 10.6c0-2.6 1.2-3.4 2.6-3.4 1 0 1.6.5 1.6 1.2" />
    </>
  ),
  // Interlocking squares: a framework of parts.
  DOTNET: (
    <>
      <rect x="2.2" y="2.2" width="6" height="6" rx="1" />
      <rect x="7.8" y="7.8" width="6" height="6" rx="1" />
    </>
  ),
  // A layered page: a site that is served, not run.
  STATIC: (
    <>
      <rect x="2.2" y="3" width="11.6" height="9" rx="1.4" />
      <path d="M2.2 6h11.6M5 3v3" />
    </>
  ),
  // A container: a box on a deck.
  DOCKERFILE: (
    <>
      <rect x="2.4" y="6.4" width="11.2" height="5.4" rx="1" />
      <path d="M5 6.4V4.2h3.2v2.2M8.2 6.4V4.2h3.2v2.2M2.4 9.1h11.2" />
    </>
  ),
  // An endpoint: a route between two ends.
  REST_API: (
    <>
      <path d="M4.4 4.6L1.8 8l2.6 3.4M11.6 4.6L14.2 8l-2.6 3.4" />
      <circle cx="8" cy="8" r="1.4" />
    </>
  ),
  // A cycle: work that comes round again.
  WORKER: (
    <>
      <path d="M13.2 8a5.2 5.2 0 01-9 3.5M2.8 8a5.2 5.2 0 019-3.5" />
      <path d="M11.8 1.9v2.8H9M4.2 14.1v-2.8H7" />
    </>
  ),
  // A bot: the Discord case, which is a running process with a face.
  DISCORD_BOT: (
    <>
      <rect x="2.6" y="5.2" width="10.8" height="7.4" rx="2.2" />
      <path d="M8 2.4v2.8M5.8 8.6v.9M10.2 8.6v.9" />
    </>
  ),
  // Overlapping planes: several toolchains at once.
  POLYGLOT: (
    <>
      <path d="M8 1.9l6 3.1-6 3.1-6-3.1z" />
      <path d="M2 8l6 3.1L14 8M2 11l6 3.1L14 11" />
    </>
  ),
};

/** Aliases, so a runtime the planner spells differently still gets its glyph
 *  rather than silently falling through to the generic one. */
const ALIASES: Record<string, string> = {
  STATIC_SITE: 'STATIC',
  WEBSITE: 'STATIC',
  NODE_APP: 'NODEJS',
  PYTHON_APP: 'PYTHON',
  DOCKER_COMPOSE: 'DOCKERFILE',
  SERVICE: 'DOCKERFILE',
};

function glyphFor(runtime: string): React.ReactNode {
  const key = runtime.toUpperCase();
  return GLYPHS[key] ?? GLYPHS[ALIASES[key] ?? ''] ?? GLYPHS.STATIC;
}

/**
 * A project's mark: its runtime glyph on its own derived colour.
 *
 * `title` is omitted rather than empty when the mark sits beside the project's
 * name, which is the usual case — announcing "api" twice is worse than not
 * announcing it once.
 */
export default function ProjectMark({
  projectId,
  runtime,
  size = 36,
  title,
}: {
  /** Anything stable and unique. The id, not the name, so renaming a project
   *  does not change the colour someone has learned to look for. */
  projectId: string;
  runtime: string;
  size?: number;
  title?: string;
}) {
  const identity = identityFor(projectId);
  // A squircle-ish radius rather than a fixed 8px: at 24px a hard 8 reads as a
  // rounded rectangle, at 48px as a circle. Scaling keeps one shape.
  const radius = Math.round(size * 0.28);

  return (
    <span
      role={title ? 'img' : undefined}
      aria-label={title}
      aria-hidden={title ? undefined : true}
      className="mark-tile grid shrink-0 place-items-center"
      style={{
        width: size,
        height: size,
        borderRadius: radius,
        backgroundImage: `linear-gradient(145deg, ${identity.from}, ${identity.to})`,
      }}
    >
      <svg
        width={Math.round(size * 0.58)}
        height={Math.round(size * 0.58)}
        viewBox="0 0 16 16"
        fill="none"
        stroke="#fff"
        strokeWidth="1.5"
        strokeLinecap="round"
        strokeLinejoin="round"
        aria-hidden
        focusable="false"
      >
        {glyphFor(runtime)}
      </svg>
    </span>
  );
}

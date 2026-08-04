/**
 * The Panel Platform mark.
 *
 * Redrawn as geometry rather than shipped as the source PNG. That file is a
 * presentation render — 1536×1024, 2 MB, a grey gradient background, an outer
 * glow and the wordmark all baked into the pixels — and at the 36px it is
 * displayed at, a downscale of it reads as a soft stamp beside crisp UI. The
 * glyph is built from sheared rounded rectangles, so it costs nothing to
 * express exactly, stays sharp at any size and on any display density, and can
 * be recoloured by the theme.
 *
 * The construction is three panel rows on a −20° shear, with the detached
 * square left upright: in the original the square is the one element that does
 * not lean, and shearing it would lose that.
 */
export default function Logo({ size = 36, title }: { size?: number; title?: string }) {
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 32 32"
      fill="none"
      xmlns="http://www.w3.org/2000/svg"
      role={title ? 'img' : undefined}
      aria-label={title}
      aria-hidden={title ? undefined : true}
    >
      <defs>
        {/* Bottom-left to top-right, following the light in the original. */}
        <linearGradient id="pp-mark" x1="0" y1="1" x2="1" y2="0">
          <stop offset="0%" stopColor="#2563eb" />
          <stop offset="100%" stopColor="#00a2ff" />
        </linearGradient>
      </defs>

      {/* The rows. The translate compensates for the shear, which lifts the
          right-hand end of every bar as x grows. */}
      <g transform="translate(0 6.6) skewY(-20)" fill="url(#pp-mark)">
        <rect x="2.4" y="2.6" width="17.2" height="6.2" rx="1.3" />

        <rect x="2.4" y="9.6" width="4" height="6.2" rx="1.1" />
        <rect x="7.3" y="9.6" width="16.6" height="6.2" rx="1.3" />

        <rect x="2.4" y="16.6" width="4" height="6.2" rx="1.1" />
        <rect x="7.3" y="16.6" width="14" height="6.2" rx="1.3" />
      </g>

      {/* Detached, and deliberately not sheared. Solid ring with a lighter
          centre rather than a stroke, so it holds its shape at 22px where a
          sub-pixel stroke would disappear. */}
      <rect x="22.9" y="0.9" width="6.4" height="6.4" rx="2" fill="#2f6fe4" />
      <rect x="24.5" y="2.5" width="3.2" height="3.2" rx="0.9" fill="#38bdf8" />
    </svg>
  );
}

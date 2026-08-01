/**
 * The workspace's icons.
 *
 * Drawn here as plain SVG rather than pulled from an icon font. A font would be
 * a network request or a bundled binary for two dozen glyphs, and the previous
 * interface used emoji and box-drawing characters, which render differently on
 * every platform and cannot be tinted.
 *
 * Everything is a 16×16 stroke drawing in `currentColor`, so an icon takes the
 * colour of whatever contains it and a hover state needs no second asset.
 */
import type { ReactNode } from 'react';

export type IconName =
  | 'chevron-right'
  | 'chevron-down'
  | 'file'
  | 'folder'
  | 'folder-open'
  | 'new-file'
  | 'new-folder'
  | 'refresh'
  | 'collapse-all'
  | 'close'
  | 'search'
  | 'source-control'
  | 'run'
  | 'extensions'
  | 'settings'
  | 'account'
  | 'terminal'
  | 'play'
  | 'stop'
  | 'restart'
  | 'power'
  | 'error'
  | 'warning'
  | 'info'
  | 'arrow-left'
  | 'arrow-right'
  | 'sidebar'
  | 'panel'
  | 'check'
  | 'trash'
  | 'pencil'
  | 'copy'
  | 'save'
  | 'external'
  | 'blocked'
  | 'output';

const PATHS: Record<IconName, ReactNode> = {
  'chevron-right': <path d="M6.5 4 10.5 8 6.5 12" />,
  'chevron-down': <path d="M4 6.5 8 10.5 12 6.5" />,
  file: (
    <>
      <path d="M4 2h5l3 3v9H4z" />
      <path d="M9 2v3h3" />
    </>
  ),
  folder: <path d="M2 12.5V3.5h4L7.5 5H14v7.5z" />,
  'folder-open': (
    <>
      <path d="M2 12.5V3.5h4L7.5 5H13v2" />
      <path d="M2 12.5 4.2 7H15l-2.2 5.5z" />
    </>
  ),
  'new-file': (
    <>
      <path d="M4 2h5l3 3v4" />
      <path d="M4 2v12h4" />
      <path d="M9 2v3h3" />
      <path d="M12 10v5M9.5 12.5h5" />
    </>
  ),
  'new-folder': (
    <>
      <path d="M2 12.5V3.5h4L7.5 5H14v3.5" />
      <path d="M2 12.5h6" />
      <path d="M12 9.5v5M9.5 12h5" />
    </>
  ),
  refresh: (
    <>
      <path d="M13 8a5 5 0 1 1-1.7-3.8" />
      <path d="M13 2.5V5h-2.5" />
    </>
  ),
  'collapse-all': (
    <>
      <path d="M5 3 8 6l3-3" />
      <path d="M5 13l3-3 3 3" />
    </>
  ),
  close: <path d="M4 4l8 8M12 4l-8 8" />,
  search: (
    <>
      <circle cx="7" cy="7" r="4.2" />
      <path d="M10.2 10.2 14 14" />
    </>
  ),
  'source-control': (
    <>
      <circle cx="5" cy="4" r="1.6" />
      <circle cx="5" cy="12" r="1.6" />
      <circle cx="11" cy="4" r="1.6" />
      <path d="M5 5.6v4.8" />
      <path d="M11 5.6v1.4a3 3 0 0 1-3 3H5" />
    </>
  ),
  run: (
    <>
      <circle cx="8" cy="8" r="5.6" />
      <path d="M6.8 5.5 10.6 8l-3.8 2.5z" />
    </>
  ),
  extensions: (
    <>
      <rect x="1.8" y="1.8" width="5.2" height="5.2" rx="0.6" />
      <rect x="9" y="1.8" width="5.2" height="5.2" rx="0.6" />
      <rect x="1.8" y="9" width="5.2" height="5.2" rx="0.6" />
      <rect x="9" y="9" width="5.2" height="5.2" rx="0.6" strokeDasharray="1.8 1.4" />
    </>
  ),
  settings: (
    <>
      <circle cx="8" cy="8" r="2.2" />
      <path d="M8 1.6v2.2M8 12.2v2.2M1.6 8h2.2M12.2 8h2.2M3.5 3.5l1.6 1.6M10.9 10.9l1.6 1.6M12.5 3.5l-1.6 1.6M5.1 10.9l-1.6 1.6" />
    </>
  ),
  account: (
    <>
      <circle cx="8" cy="5.8" r="2.7" />
      <path d="M3 14a5 5 0 0 1 10 0" />
    </>
  ),
  terminal: (
    <>
      <rect x="1.5" y="2.5" width="13" height="11" rx="1" />
      <path d="M4.5 6.2 7 8.5l-2.5 2.3" />
      <path d="M8.6 11h3.2" />
    </>
  ),
  play: <path d="M5 3l8 5-8 5z" fill="currentColor" stroke="none" />,
  stop: <rect x="4" y="4" width="8" height="8" rx="0.8" fill="currentColor" stroke="none" />,
  restart: (
    <>
      <path d="M3 8a5 5 0 1 0 1.7-3.8" />
      <path d="M3 2.5V5h2.5" />
    </>
  ),
  power: (
    <>
      <path d="M8 2v6" />
      <path d="M11.8 4.4a5.5 5.5 0 1 1-7.6 0" />
    </>
  ),
  error: (
    <>
      <circle cx="8" cy="8" r="6" />
      <path d="M5.9 5.9l4.2 4.2M10.1 5.9l-4.2 4.2" />
    </>
  ),
  warning: (
    <>
      <path d="M8 2.4 14.8 13.6H1.2z" />
      <path d="M8 6.4v3.4" />
      <path d="M8 11.6v.6" />
    </>
  ),
  info: (
    <>
      <circle cx="8" cy="8" r="6" />
      <path d="M8 7.2v4M8 4.8v.6" />
    </>
  ),
  'arrow-left': <path d="M10 3 5 8l5 5" />,
  'arrow-right': <path d="M6 3l5 5-5 5" />,
  sidebar: (
    <>
      <rect x="1.5" y="2.5" width="13" height="11" rx="1" />
      <path d="M6 2.5v11" />
    </>
  ),
  panel: (
    <>
      <rect x="1.5" y="2.5" width="13" height="11" rx="1" />
      <path d="M1.5 9.5h13" />
    </>
  ),
  check: <path d="M3.2 8.4 6.4 11.8 12.8 4.6" />,
  trash: (
    <>
      <path d="M2.8 4.3h10.4" />
      <path d="M6.3 4.3V2.8h3.4v1.5" />
      <path d="M4.4 4.3 5.1 13.4h5.8l.7-9.1" />
    </>
  ),
  pencil: <path d="M2.8 13.2v-2.4L10.4 3.2l2.4 2.4-7.6 7.6z" />,
  copy: (
    <>
      <rect x="5.2" y="5.2" width="8.3" height="8.3" rx="0.8" />
      <path d="M10.8 5.2V3.3a.8.8 0 0 0-.8-.8H3.3a.8.8 0 0 0-.8.8V10a.8.8 0 0 0 .8.8h1.9" />
    </>
  ),
  save: (
    <>
      <path d="M2.5 2.5h8.6L13.5 5v8.5h-11z" />
      <path d="M5 2.5v3.8h5V2.5" />
      <path d="M4.6 13.5V9.4h6.8v4.1" />
    </>
  ),
  external: (
    <>
      <path d="M13 8.5v4.2a.8.8 0 0 1-.8.8H3.3a.8.8 0 0 1-.8-.8V3.8a.8.8 0 0 1 .8-.8h4.2" />
      <path d="M10 2.5h3.5V6" />
      <path d="M7.5 8.5 13.4 2.6" />
    </>
  ),
  blocked: (
    <>
      <circle cx="8" cy="8" r="6" />
      <path d="M3.8 3.8l8.4 8.4" />
    </>
  ),
  output: (
    <>
      <rect x="1.5" y="2.5" width="13" height="11" rx="1" />
      <path d="M4.2 6h7.6M4.2 8.6h5.4M4.2 11.2h6.4" />
    </>
  ),
};

/**
 * One icon.
 *
 * `aria-hidden` by default: an icon next to a label is decoration, and the few
 * icon-only buttons in the workspace carry their own `aria-label` and `title`.
 */
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
      strokeWidth="1.2"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={`shrink-0 ${className}`}
    >
      {PATHS[name]}
    </svg>
  );
}

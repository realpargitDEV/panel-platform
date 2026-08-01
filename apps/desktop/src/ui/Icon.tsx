/**
 * The application's icons.
 *
 * Plain SVG rather than an icon font: a font would be a network request the CSP
 * forbids, or a binary bundled into every install to draw two dozen glyphs. All
 * of them are 16×16 stroke drawings in `currentColor`, so an icon takes the
 * colour of whatever contains it and a hover state needs no second asset.
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
  overview: (
    <>
      <rect x="2" y="2" width="5.5" height="5.5" rx="1.2" />
      <rect x="8.5" y="2" width="5.5" height="5.5" rx="1.2" />
      <rect x="2" y="8.5" width="5.5" height="5.5" rx="1.2" />
      <rect x="8.5" y="8.5" width="5.5" height="5.5" rx="1.2" />
    </>
  ),
  projects: (
    <>
      <path d="M8 1.8 14.2 5v6L8 14.2 1.8 11V5z" />
      <path d="M1.8 5 8 8.2 14.2 5" />
      <path d="M8 8.2v6" />
    </>
  ),
  activity: <path d="M1.5 8.5h3l2-5 3 9 2-4h3" />,
  discord: (
    <>
      <path d="M5.8 11.8c-1.6-.5-2.8-1.6-3.3-3 .4-2.6 1.5-4.6 3-5.6l.8 1.3a7 7 0 0 1 3.4 0l.8-1.3c1.5 1 2.6 3 3 5.6-.5 1.4-1.7 2.5-3.3 3l-.7-1.2" />
      <circle cx="6.2" cy="8" r="0.9" />
      <circle cx="9.8" cy="8" r="0.9" />
    </>
  ),
  settings: (
    <>
      <circle cx="8" cy="8" r="2.2" />
      <path d="M8 1.6v2.2M8 12.2v2.2M1.6 8h2.2M12.2 8h2.2M3.5 3.5l1.6 1.6M10.9 10.9l1.6 1.6M12.5 3.5l-1.6 1.6M5.1 10.9l-1.6 1.6" />
    </>
  ),
  plus: <path d="M8 3.2v9.6M3.2 8h9.6" />,
  search: (
    <>
      <circle cx="7" cy="7" r="4.2" />
      <path d="M10.2 10.2 14 14" />
    </>
  ),
  'chevron-down': <path d="M4 6.5 8 10.5 12 6.5" />,
  'chevron-right': <path d="M6.5 4 10.5 8 6.5 12" />,
  'chevron-left': <path d="M9.5 4 5.5 8 9.5 12" />,
  close: <path d="M4 4l8 8M12 4l-8 8" />,
  alert: (
    <>
      <path d="M8 2.4 14.8 13.6H1.2z" />
      <path d="M8 6.4v3.4M8 11.6v.6" />
    </>
  ),
  info: (
    <>
      <circle cx="8" cy="8" r="6" />
      <path d="M8 7.2v4M8 4.8v.6" />
    </>
  ),
  check: <path d="M3.2 8.4 6.4 11.8 12.8 4.6" />,
  'check-circle': (
    <>
      <circle cx="8" cy="8" r="6" />
      <path d="M5.2 8.2 7.2 10.4l3.6-4" />
    </>
  ),
  play: <path d="M5 3l8 5-8 5z" fill="currentColor" stroke="none" />,
  stop: <rect x="4" y="4" width="8" height="8" rx="1" fill="currentColor" stroke="none" />,
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
  refresh: (
    <>
      <path d="M13 8a5 5 0 1 1-1.7-3.8" />
      <path d="M13 2.5V5h-2.5" />
    </>
  ),
  external: (
    <>
      <path d="M13 8.5v4.2a.8.8 0 0 1-.8.8H3.3a.8.8 0 0 1-.8-.8V3.8a.8.8 0 0 1 .8-.8h4.2" />
      <path d="M10 2.5h3.5V6" />
      <path d="M7.5 8.5 13.4 2.6" />
    </>
  ),
  folder: <path d="M2 12.5V3.5h4L7.5 5H14v7.5z" />,
  file: (
    <>
      <path d="M4 2h5l3 3v9H4z" />
      <path d="M9 2v3h3" />
    </>
  ),
  git: (
    <>
      <circle cx="5" cy="4" r="1.6" />
      <circle cx="5" cy="12" r="1.6" />
      <circle cx="11" cy="4" r="1.6" />
      <path d="M5 5.6v4.8" />
      <path d="M11 5.6v1.4a3 3 0 0 1-3 3H5" />
    </>
  ),
  download: (
    <>
      <path d="M8 2.5v8" />
      <path d="M4.8 7.5 8 10.7l3.2-3.2" />
      <path d="M2.5 13.5h11" />
    </>
  ),
  upload: (
    <>
      <path d="M8 13.5v-8" />
      <path d="M4.8 8.5 8 5.3l3.2 3.2" />
      <path d="M2.5 2.5h11" />
    </>
  ),
  trash: (
    <>
      <path d="M2.8 4.3h10.4" />
      <path d="M6.3 4.3V2.8h3.4v1.5" />
      <path d="M4.4 4.3 5.1 13.4h5.8l.7-9.1" />
    </>
  ),
  more: (
    <>
      <circle cx="3.5" cy="8" r="1.1" fill="currentColor" stroke="none" />
      <circle cx="8" cy="8" r="1.1" fill="currentColor" stroke="none" />
      <circle cx="12.5" cy="8" r="1.1" fill="currentColor" stroke="none" />
    </>
  ),
  grid: (
    <>
      <rect x="2" y="2" width="5.5" height="5.5" rx="1.2" />
      <rect x="8.5" y="2" width="5.5" height="5.5" rx="1.2" />
      <rect x="2" y="8.5" width="5.5" height="5.5" rx="1.2" />
      <rect x="8.5" y="8.5" width="5.5" height="5.5" rx="1.2" />
    </>
  ),
  list: <path d="M2.5 4h11M2.5 8h11M2.5 12h11" />,
  filter: <path d="M2.2 3.5h11.6L9.4 8.4v4.3l-2.8 1.3V8.4z" />,
  cpu: (
    <>
      <rect x="4.5" y="4.5" width="7" height="7" rx="1.2" />
      <path d="M6.5 2v2.5M9.5 2v2.5M6.5 11.5V14M9.5 11.5V14M2 6.5h2.5M2 9.5h2.5M11.5 6.5H14M11.5 9.5H14" />
    </>
  ),
  memory: (
    <>
      <rect x="2" y="4.5" width="12" height="7" rx="1.2" />
      <path d="M5 7.5v2.5M8 7.5v2.5M11 7.5v2.5" />
    </>
  ),
  disk: (
    <>
      <ellipse cx="8" cy="4" rx="5.5" ry="2.2" />
      <path d="M2.5 4v8c0 1.2 2.5 2.2 5.5 2.2s5.5-1 5.5-2.2V4" />
      <path d="M2.5 8c0 1.2 2.5 2.2 5.5 2.2s5.5-1 5.5-2.2" />
    </>
  ),
  clock: (
    <>
      <circle cx="8" cy="8" r="6" />
      <path d="M8 4.5V8l2.5 1.5" />
    </>
  ),
  network: (
    <>
      <circle cx="8" cy="8" r="6" />
      <path d="M2 8h12" />
      <path d="M8 2c1.8 2 2.7 4 2.7 6s-.9 4-2.7 6c-1.8-2-2.7-4-2.7-6s.9-4 2.7-6z" />
    </>
  ),
  terminal: (
    <>
      <rect x="1.5" y="2.5" width="13" height="11" rx="1.5" />
      <path d="M4.5 6.2 7 8.5l-2.5 2.3" />
      <path d="M8.6 11h3.2" />
    </>
  ),
  logs: (
    <>
      <rect x="2.5" y="1.8" width="11" height="12.4" rx="1.5" />
      <path d="M5.2 5.2h5.6M5.2 8h5.6M5.2 10.8h3.4" />
    </>
  ),
  shield: (
    <>
      <path d="M8 1.8 13.2 4v4c0 3-2.2 5.4-5.2 6.2C5 13.4 2.8 11 2.8 8V4z" />
      <path d="M5.8 8.1 7.3 9.6l3-3.2" />
    </>
  ),
  bell: (
    <>
      <path d="M4.2 6.6a3.8 3.8 0 0 1 7.6 0c0 3 1.2 4.2 1.2 4.2H3s1.2-1.2 1.2-4.2z" />
      <path d="M6.6 13a1.6 1.6 0 0 0 2.8 0" />
    </>
  ),
  user: (
    <>
      <circle cx="8" cy="5.8" r="2.7" />
      <path d="M3 14a5 5 0 0 1 10 0" />
    </>
  ),
  sidebar: (
    <>
      <rect x="1.5" y="2.5" width="13" height="11" rx="1.5" />
      <path d="M6 2.5v11" />
    </>
  ),
  copy: (
    <>
      <rect x="5.2" y="5.2" width="8.3" height="8.3" rx="1.2" />
      <path d="M10.8 5.2V3.3a.8.8 0 0 0-.8-.8H3.3a.8.8 0 0 0-.8.8V10a.8.8 0 0 0 .8.8h1.9" />
    </>
  ),
  edit: <path d="M2.8 13.2v-2.4L10.4 3.2l2.4 2.4-7.6 7.6z" />,
  container: (
    <>
      <rect x="2" y="5.5" width="12" height="7.5" rx="1.2" />
      <path d="M5 5.5V3.5h6v2" />
      <path d="M5.5 8.5h5" />
    </>
  ),
  'arrow-right': (
    <>
      <path d="M2.5 8h11" />
      <path d="M9.5 4 13.5 8l-4 4" />
    </>
  ),
  command: (
    <path d="M5.5 2.5a1.8 1.8 0 1 0 1.8 1.8v7.4a1.8 1.8 0 1 0 1.8-1.8H4.3a1.8 1.8 0 1 0 1.8 1.8V4.3a1.8 1.8 0 0 0-.6-1.8z" />
  ),
  blocked: (
    <>
      <circle cx="8" cy="8" r="6" />
      <path d="M3.8 3.8l8.4 8.4" />
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
      strokeWidth="1.3"
      strokeLinecap="round"
      strokeLinejoin="round"
      className={`shrink-0 ${className}`}
    >
      {PATHS[name]}
    </svg>
  );
}

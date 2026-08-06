/**
 * Retro & Historical.
 *
 * Where the shape tokens earn their place. These interfaces are remembered for
 * their edges and their type as much as their colour — square corners, two-pixel
 * borders, a hard shadow with no blur, a system face that is not the one the
 * rest of the application uses. A recolour alone would make every one of these
 * read as "grey theme".
 *
 * The three named after products are named for what they are instead:
 * `System 95`, `System XP` and `Classic Desktop`.
 */

import { SERIF_DISPLAY, SERIF_NEWS, SYSTEM_CLASSIC, SYSTEM_LEGACY, TERMINAL } from '../fonts';
import type { Theme } from '../types';

export const RETRO_THEMES: readonly Theme[] = [
  {
    id: 'system-95',
    name: 'System 95',
    category: 'retro',
    detail: 'Grey chrome, navy title bars, hard shadows and square everything.',
    tokens: {
      canvas: '#b8b8b8',
      surface: '#d4d0c8',
      raised: '#c8c4bc',
      overlay: '#dfdcd6',
      edge: '#808080',
      edgeStrong: '#404040',
      ink: '#000000',
      muted: '#3a3a3a',
      accent: '#000080',
    },
    traits: {
      fontUi: SYSTEM_LEGACY,
      radiusScale: 0,
      borderWidth: '2px',
      shadowCard: '2px 2px 0 rgb(0 0 0 / 0.35)',
      shadowRaised: '1px 1px 0 rgb(0 0 0 / 0.3)',
    },
  },
  {
    id: 'system-xp',
    name: 'System XP',
    category: 'retro',
    detail: 'Cream panels, a bright blue task strip and softly rounded controls.',
    tokens: {
      canvas: '#ece9d8',
      surface: '#ffffff',
      raised: '#f3f1e6',
      overlay: '#ffffff',
      edge: '#c4c0aa',
      edgeStrong: '#8e8a76',
      ink: '#1a1a1a',
      muted: '#4b4b4b',
      accent: '#245edb',
    },
    traits: {
      fontUi: SYSTEM_LEGACY,
      radiusScale: 0.8,
    },
  },
  {
    id: 'classic-desktop',
    name: 'Classic Desktop',
    category: 'retro',
    detail: 'Monochrome windows, black hairlines, no colour anywhere.',
    tokens: {
      canvas: '#dcdcdc',
      surface: '#ffffff',
      raised: '#eeeeee',
      overlay: '#ffffff',
      edge: '#8a8a8a',
      edgeStrong: '#000000',
      ink: '#000000',
      muted: '#3a3a3a',
      accent: '#2b2b2b',
    },
    traits: {
      fontUi: SYSTEM_CLASSIC,
      radiusScale: 0,
      shadowCard: '1px 1px 0 rgb(0 0 0 / 0.6)',
    },
  },
  {
    id: 'dos',
    name: 'DOS',
    category: 'retro',
    detail: 'Black screen, bright green text, no shapes at all.',
    tokens: {
      canvas: '#000000',
      surface: '#07070c',
      raised: '#0c0c14',
      overlay: '#12121d',
      edge: '#1e1e2e',
      edgeStrong: '#33334f',
      ink: '#e0e0e0',
      muted: '#b4b4b4',
      accent: '#55ff55',
      ok: '#55ff55',
      warn: '#ffff55',
      danger: '#ff5555',
    },
    traits: {
      fontUi: TERMINAL,
      fontMono: TERMINAL,
      radiusScale: 0,
    },
  },
  {
    id: 'crt-monitor',
    name: 'CRT Monitor',
    category: 'retro',
    detail: 'Phosphor green behind scan lines, with the glow of a warm tube.',
    tokens: {
      canvas: '#0a0f0a',
      surface: '#0f170f',
      raised: '#141d14',
      overlay: '#1a251a',
      edge: '#223022',
      edgeStrong: '#385038',
      ink: '#d8ffd8',
      muted: '#95c995',
      accent: '#4ade80',
      ok: '#4ade80',
    },
    traits: {
      fontUi: TERMINAL,
      fontMono: TERMINAL,
      radiusScale: 0,
      glow: '#4ade80',
    },
    effect: 'scanlines',
  },
  {
    id: 'vaporwave',
    name: 'Vaporwave',
    category: 'retro',
    detail: 'Hot pink and cyan over a violet grid running to the horizon.',
    tokens: {
      canvas: '#1a0b2e',
      surface: '#24103d',
      raised: '#2c144a',
      overlay: '#351858',
      edge: '#421f6b',
      edgeStrong: '#6634a3',
      ink: '#ffe6fb',
      muted: '#d3aadc',
      accent: '#ff71ce',
      ok: '#05ffa1',
      warn: '#fffb96',
    },
    traits: {
      glow: '#ff71ce',
    },
    effect: 'grid',
  },
  {
    id: 'synthwave',
    name: 'Synthwave',
    category: 'retro',
    detail: 'Neon sunset magenta over a dark grid. Louder than Vaporwave.',
    tokens: {
      canvas: '#16082a',
      surface: '#1f0c3a',
      raised: '#271047',
      overlay: '#2f1455',
      edge: '#3b1a68',
      edgeStrong: '#5e2fa3',
      ink: '#ffeaf7',
      muted: '#cfa8dc',
      accent: '#f92aad',
      warn: '#ff8a3d',
    },
    traits: {
      glow: '#f92aad',
    },
    effect: 'grid',
  },
  {
    id: 'cassette-era',
    name: 'Cassette Era',
    category: 'retro',
    detail: 'Beige hi-fi panels with orange level meters.',
    tokens: {
      canvas: '#1a1713',
      surface: '#241f19',
      raised: '#2c261e',
      overlay: '#342d24',
      edge: '#40372c',
      edgeStrong: '#645643',
      ink: '#f3e9da',
      muted: '#c7b59b',
      accent: '#ff8c42',
    },
    traits: {
      radiusScale: 0.5,
    },
  },
  {
    id: 'old-newspaper',
    name: 'Old Newspaper',
    category: 'retro',
    detail: 'Faded stock, black ink, a narrow serif and a halftone screen.',
    tokens: {
      canvas: '#ece7db',
      surface: '#f7f3e9',
      raised: '#e4dfd2',
      overlay: '#f7f3e9',
      edge: '#cec8b8',
      edgeStrong: '#97927f',
      ink: '#14120e',
      muted: '#494539',
      accent: '#8b2b1f',
    },
    traits: {
      fontUi: SERIF_NEWS,
      radiusScale: 0,
    },
    effect: 'halftone',
  },
  {
    id: 'victorian-machine',
    name: 'Victorian Machine',
    category: 'retro',
    detail: 'Dark wood and brass, with gauges rather than indicators.',
    tokens: {
      canvas: '#120d09',
      surface: '#1b140e',
      raised: '#221a12',
      overlay: '#2a2117',
      edge: '#34281b',
      edgeStrong: '#56432c',
      ink: '#f3e8d2',
      muted: '#c7ad84',
      accent: '#c08a2e',
    },
    traits: {
      fontUi: SERIF_DISPLAY,
      radiusScale: 0.6,
    },
  },
];

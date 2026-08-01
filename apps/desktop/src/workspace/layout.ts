/**
 * Where the panels are, and how big.
 *
 * Kept apart from the components so the rules that are easy to get wrong — a
 * drag that would leave the sidebar two pixels wide, a stored layout written by
 * an older version, a window shorter than the panel that was saved into it — are
 * pure functions with tests rather than arithmetic buried in a mousemove
 * handler.
 *
 * Sizes are persisted, because a layout that resets every time the window opens
 * is a layout nobody bothers to arrange.
 */

/** Which sidebar the activity bar is showing. */
export type ActivityView =
  'explorer' | 'search' | 'source-control' | 'run' | 'extensions' | 'account';

/** Which tab the bottom panel is showing. */
export type PanelTab = 'problems' | 'output' | 'terminal' | 'logs';

export interface Layout {
  sidebarWidth: number;
  panelHeight: number;
  sidebarVisible: boolean;
  panelVisible: boolean;
  activityView: ActivityView;
  panelTab: PanelTab;
}

export const MIN_SIDEBAR_WIDTH = 170;
export const MAX_SIDEBAR_WIDTH = 640;
export const MIN_PANEL_HEIGHT = 80;

export const defaultLayout: Layout = {
  sidebarWidth: 260,
  panelHeight: 220,
  sidebarVisible: true,
  panelVisible: false,
  activityView: 'explorer',
  panelTab: 'problems',
};

const STORAGE_KEY = 'workspace.layout.v1';

const ACTIVITY_VIEWS: ActivityView[] = [
  'explorer',
  'search',
  'source-control',
  'run',
  'extensions',
  'account',
];
const PANEL_TABS: PanelTab[] = ['problems', 'output', 'terminal', 'logs'];

/**
 * The sidebar can be dragged narrow enough to be useless, or wider than the
 * window. Neither is a state the user can recover from by dragging, because the
 * handle goes with it.
 */
export function clampSidebarWidth(width: number, windowWidth: number): number {
  const ceiling = Math.max(MIN_SIDEBAR_WIDTH, Math.min(MAX_SIDEBAR_WIDTH, windowWidth - 320));
  return Math.round(Math.max(MIN_SIDEBAR_WIDTH, Math.min(ceiling, width)));
}

/**
 * The bottom panel never grows past the point where the editor would disappear:
 * 180px of editor is kept back whatever the drag asks for.
 */
export function clampPanelHeight(height: number, windowHeight: number): number {
  const ceiling = Math.max(MIN_PANEL_HEIGHT, windowHeight - 180);
  return Math.round(Math.max(MIN_PANEL_HEIGHT, Math.min(ceiling, height)));
}

/** The subset of `Storage` this module uses. Narrow enough to fake in a test. */
export interface LayoutStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

/**
 * Read the stored layout, falling back to the default for anything missing or
 * malformed.
 *
 * Deliberately field by field rather than a shape check: a layout written by an
 * older version is worth half-keeping, and a corrupt one must never stop the
 * workspace from opening.
 */
export function loadLayout(storage: LayoutStorage | undefined): Layout {
  const raw = storage?.getItem(STORAGE_KEY);
  if (!raw) return defaultLayout;

  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return defaultLayout;
  }
  if (parsed === null || typeof parsed !== 'object') return defaultLayout;

  const stored = parsed as Partial<Record<keyof Layout, unknown>>;
  return {
    sidebarWidth: number(stored.sidebarWidth, defaultLayout.sidebarWidth),
    panelHeight: number(stored.panelHeight, defaultLayout.panelHeight),
    sidebarVisible: boolean(stored.sidebarVisible, defaultLayout.sidebarVisible),
    panelVisible: boolean(stored.panelVisible, defaultLayout.panelVisible),
    activityView: member(stored.activityView, ACTIVITY_VIEWS, defaultLayout.activityView),
    panelTab: member(stored.panelTab, PANEL_TABS, defaultLayout.panelTab),
  };
}

/** Write the layout. A storage that refuses (private mode, quota) is ignored. */
export function saveLayout(storage: LayoutStorage | undefined, layout: Layout): void {
  try {
    storage?.setItem(STORAGE_KEY, JSON.stringify(layout));
  } catch {
    // Losing a remembered panel size is not worth an error in front of anyone.
  }
}

function number(value: unknown, fallback: number): number {
  return typeof value === 'number' && Number.isFinite(value) ? value : fallback;
}

function boolean(value: unknown, fallback: boolean): boolean {
  return typeof value === 'boolean' ? value : fallback;
}

function member<T extends string>(value: unknown, allowed: T[], fallback: T): T {
  return typeof value === 'string' && (allowed as string[]).includes(value)
    ? (value as T)
    : fallback;
}

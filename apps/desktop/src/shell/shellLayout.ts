/**
 * Where the application's own panels are, and how big.
 *
 * The same shape as `workspace/layout.ts`, and deliberately not shared with it:
 * the editor's layout and the shell's are two different arrangements that
 * happen to have similar fields, and folding them together would mean a change
 * to one silently resizing the other.
 *
 * Sizes are persisted for the reason that file gives — a layout that resets
 * every launch is a layout nobody bothers to arrange.
 */

/** Which tool the activity rail is showing. */
export type ToolId =
  | 'projects'
  | 'processes'
  | 'console'
  | 'ports'
  | 'environment'
  | 'resources'
  | 'discord'
  | 'settings';

export const TOOLS: ToolId[] = [
  'projects',
  'processes',
  'console',
  'ports',
  'environment',
  'resources',
  'discord',
  'settings',
];

export interface ShellLayout {
  sidebarWidth: number;
  sidebarVisible: boolean;
  tool: ToolId;
  /** Which project the workspace is pointed at, or null for none. */
  projectId: string | null;
}

export const MIN_SIDEBAR = 180;
export const MAX_SIDEBAR = 420;
export const DEFAULT_SIDEBAR = 248;

export const defaultShellLayout: ShellLayout = {
  sidebarWidth: DEFAULT_SIDEBAR,
  sidebarVisible: true,
  tool: 'projects',
  projectId: null,
};

const STORAGE_KEY = 'shell.layout.v1';

/**
 * The sidebar can be dragged narrow enough to be useless, or wide enough to
 * leave no workspace. Neither is recoverable by dragging, because the handle
 * goes with it — so both ends are clamped, and the ceiling also keeps 360px of
 * workspace on a narrow window.
 */
export function clampSidebar(width: number, windowWidth: number): number {
  const ceiling = Math.max(MIN_SIDEBAR, Math.min(MAX_SIDEBAR, windowWidth - 360));
  return Math.round(Math.max(MIN_SIDEBAR, Math.min(ceiling, width)));
}

/** The subset of `Storage` this module uses. Narrow enough to fake in a test. */
export interface LayoutStorage {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
}

/**
 * Read the stored layout, falling back field by field.
 *
 * Field by field rather than a shape check, for the reason the editor's loader
 * gives: a layout written by an older version is worth half-keeping, and a
 * corrupt one must never stop the window from opening.
 */
export function loadShellLayout(storage: LayoutStorage | undefined): ShellLayout {
  const raw = storage?.getItem(STORAGE_KEY);
  if (!raw) return defaultShellLayout;

  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return defaultShellLayout;
  }
  if (parsed === null || typeof parsed !== 'object') return defaultShellLayout;

  const stored = parsed as Partial<Record<keyof ShellLayout, unknown>>;
  return {
    sidebarWidth:
      typeof stored.sidebarWidth === 'number' && Number.isFinite(stored.sidebarWidth)
        ? stored.sidebarWidth
        : defaultShellLayout.sidebarWidth,
    sidebarVisible:
      typeof stored.sidebarVisible === 'boolean'
        ? stored.sidebarVisible
        : defaultShellLayout.sidebarVisible,
    tool:
      typeof stored.tool === 'string' && (TOOLS as string[]).includes(stored.tool)
        ? (stored.tool as ToolId)
        : defaultShellLayout.tool,
    // Not validated against the project list here: this module cannot know
    // what exists, and the shell drops an id that no longer resolves.
    projectId: typeof stored.projectId === 'string' ? stored.projectId : null,
  };
}

/** Write the layout. A storage that refuses (private mode, quota) is ignored. */
export function saveShellLayout(storage: LayoutStorage | undefined, layout: ShellLayout): void {
  try {
    storage?.setItem(STORAGE_KEY, JSON.stringify(layout));
  } catch {
    // Losing a remembered width is not worth an error in front of anyone.
  }
}

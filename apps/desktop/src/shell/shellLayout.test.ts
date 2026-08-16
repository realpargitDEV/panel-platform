import { describe, expect, it } from 'vitest';

import {
  clampSidebar,
  defaultShellLayout,
  loadShellLayout,
  MAX_SIDEBAR,
  MIN_SIDEBAR,
  saveShellLayout,
  type LayoutStorage,
} from './shellLayout';

function storage(initial: Record<string, string> = {}): LayoutStorage & { map: Map<string, string> } {
  const map = new Map(Object.entries(initial));
  return {
    map,
    getItem: (key) => map.get(key) ?? null,
    setItem: (key, value) => {
      map.set(key, value);
    },
  };
}

describe('clampSidebar', () => {
  it('keeps a drag inside the usable range', () => {
    expect(clampSidebar(10, 1920)).toBe(MIN_SIDEBAR);
    expect(clampSidebar(9999, 1920)).toBe(MAX_SIDEBAR);
    expect(clampSidebar(300, 1920)).toBe(300);
  });

  /** On a narrow window the ceiling has to leave a workspace behind. */
  it('leaves room for the workspace on a small window', () => {
    expect(clampSidebar(9999, 700)).toBe(340);
  });

  /** Even absurdly narrow: a sidebar you cannot see cannot be dragged back. */
  it('never returns less than the minimum however small the window', () => {
    expect(clampSidebar(9999, 200)).toBe(MIN_SIDEBAR);
    expect(clampSidebar(0, 200)).toBe(MIN_SIDEBAR);
  });
});

describe('loadShellLayout', () => {
  it('is the default with nothing stored', () => {
    expect(loadShellLayout(storage())).toEqual(defaultShellLayout);
    expect(loadShellLayout(undefined)).toEqual(defaultShellLayout);
  });

  it('survives corrupt json rather than refusing to open', () => {
    expect(loadShellLayout(storage({ 'shell.layout.v1': '{not json' }))).toEqual(
      defaultShellLayout,
    );
    expect(loadShellLayout(storage({ 'shell.layout.v1': 'null' }))).toEqual(defaultShellLayout);
  });

  /** A layout written by an older version is worth half-keeping. */
  it('keeps the fields it recognises and defaults the rest', () => {
    const layout = loadShellLayout(
      storage({ 'shell.layout.v1': JSON.stringify({ sidebarWidth: 300, unknown: 'x' }) }),
    );
    expect(layout.sidebarWidth).toBe(300);
    expect(layout.tool).toBe(defaultShellLayout.tool);
    expect(layout.sidebarVisible).toBe(defaultShellLayout.sidebarVisible);
  });

  it('refuses a tool id it does not have', () => {
    const layout = loadShellLayout(
      storage({ 'shell.layout.v1': JSON.stringify({ tool: 'cryptomining' }) }),
    );
    expect(layout.tool).toBe(defaultShellLayout.tool);
  });

  it('round-trips through save', () => {
    const store = storage();
    const layout = { ...defaultShellLayout, sidebarWidth: 321, tool: 'ports' as const };
    saveShellLayout(store, layout);
    expect(loadShellLayout(store)).toEqual(layout);
  });

  it('ignores a storage that refuses to write', () => {
    const refusing: LayoutStorage = {
      getItem: () => null,
      setItem: () => {
        throw new Error('quota');
      },
    };
    expect(() => saveShellLayout(refusing, defaultShellLayout)).not.toThrow();
  });
});

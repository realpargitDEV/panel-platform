import { describe, expect, it } from 'vitest';

import {
  clampPanelHeight,
  clampSidebarWidth,
  defaultLayout,
  loadLayout,
  MAX_SIDEBAR_WIDTH,
  MIN_PANEL_HEIGHT,
  MIN_SIDEBAR_WIDTH,
  saveLayout,
  type LayoutStorage,
} from './layout';

function storage(initial?: string): LayoutStorage & { written: string | null } {
  let value = initial ?? null;
  return {
    written: null,
    getItem: () => value,
    setItem(_key, next) {
      value = next;
      this.written = next;
    },
  };
}

describe('sizing the sidebar', () => {
  it('never lets a drag make it unusably narrow', () => {
    expect(clampSidebarWidth(0, 1600)).toBe(MIN_SIDEBAR_WIDTH);
    expect(clampSidebarWidth(-500, 1600)).toBe(MIN_SIDEBAR_WIDTH);
  });

  it('caps it so the editor keeps room even on a wide screen', () => {
    expect(clampSidebarWidth(5000, 3000)).toBe(MAX_SIDEBAR_WIDTH);
  });

  it('leaves the editor 320px on a narrow window', () => {
    expect(clampSidebarWidth(900, 800)).toBe(480);
  });

  it('still returns the minimum when the window is narrower than the minimum', () => {
    // A window this small cannot honour both bounds. The sidebar staying
    // usable is the one that matters: the editor can scroll, a 20px tree
    // cannot be read.
    expect(clampSidebarWidth(400, 300)).toBe(MIN_SIDEBAR_WIDTH);
  });

  it('rounds, so a fractional drag does not produce a fractional layout', () => {
    expect(clampSidebarWidth(300.4, 1600)).toBe(300);
  });
});

describe('sizing the bottom panel', () => {
  it('keeps at least a screenful of editor above it', () => {
    expect(clampPanelHeight(5000, 800)).toBe(620);
  });

  it('never collapses below the height of its own tab strip', () => {
    expect(clampPanelHeight(10, 800)).toBe(MIN_PANEL_HEIGHT);
  });

  it('falls back to the minimum when the window is shorter than the reserve', () => {
    expect(clampPanelHeight(400, 100)).toBe(MIN_PANEL_HEIGHT);
  });
});

describe('remembering the layout', () => {
  it('uses the defaults when nothing has been stored', () => {
    expect(loadLayout(storage())).toEqual(defaultLayout);
  });

  it('uses the defaults when there is no storage at all', () => {
    expect(loadLayout(undefined)).toEqual(defaultLayout);
  });

  it('reads back what was written', () => {
    const store = storage();
    const layout = {
      ...defaultLayout,
      sidebarWidth: 333,
      panelVisible: true,
      panelTab: 'terminal' as const,
      activityView: 'search' as const,
    };
    saveLayout(store, layout);
    expect(loadLayout(store)).toEqual(layout);
  });

  it('survives a corrupt entry rather than refusing to open', () => {
    expect(loadLayout(storage('not json at all'))).toEqual(defaultLayout);
    expect(loadLayout(storage('"a string"'))).toEqual(defaultLayout);
    expect(loadLayout(storage('null'))).toEqual(defaultLayout);
  });

  it('keeps the fields an older version wrote and defaults the rest', () => {
    const stored = loadLayout(storage(JSON.stringify({ sidebarWidth: 400 })));
    expect(stored.sidebarWidth).toBe(400);
    expect(stored.panelHeight).toBe(defaultLayout.panelHeight);
  });

  it('rejects a view name it does not know', () => {
    // A stored name from a build where the section existed must not select a
    // sidebar this build cannot render.
    const stored = loadLayout(storage(JSON.stringify({ activityView: 'timeline' })));
    expect(stored.activityView).toBe(defaultLayout.activityView);
  });

  it('ignores a storage that refuses to write', () => {
    const refusing: LayoutStorage = {
      getItem: () => null,
      setItem() {
        throw new Error('quota exceeded');
      },
    };
    expect(() => saveLayout(refusing, defaultLayout)).not.toThrow();
  });
});

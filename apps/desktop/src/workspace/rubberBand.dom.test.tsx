/**
 * The selection rectangle, as rendered.
 *
 * jsdom gives every element a zero-sized box, so the row rectangles are mocked
 * — that is the only way to test this without a real browser, and the geometry
 * itself is covered by `rubberBand.test.ts`. What these tests are actually for
 * is the wiring: that a press on empty space starts a rectangle, that a press
 * on a row does not, that the listeners come back down, and that the content
 * coordinates account for scrolling.
 */
import { fireEvent, render, screen } from '@testing-library/react';
import { useState } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';

import SelectionSurface from './SelectionSurface';

/** Five 22px rows stacked from the top of a 100px-tall container. */
const ROWS = ['a', 'b', 'c', 'd', 'e'];
const ROW_HEIGHT = 22;
const CONTAINER_HEIGHT = 100;

/**
 * jsdom reports every box as 0×0. Each element is given the box it would have
 * in a browser, keyed by what it is.
 */
function mockBoxes(scrollTop = 0) {
  return vi.spyOn(Element.prototype, 'getBoundingClientRect').mockImplementation(function (
    this: Element,
  ) {
    const element = this as HTMLElement;
    const path = element.dataset?.path;
    if (path !== undefined) {
      const index = ROWS.indexOf(path);
      // Viewport position: content position minus how far it is scrolled.
      const top = index * ROW_HEIGHT - scrollTop;
      return {
        top,
        bottom: top + ROW_HEIGHT,
        left: 0,
        right: 240,
        width: 240,
        height: ROW_HEIGHT,
        x: 0,
        y: top,
        toJSON: () => ({}),
      } as DOMRect;
    }
    return {
      top: 0,
      bottom: CONTAINER_HEIGHT,
      left: 0,
      right: 240,
      width: 240,
      height: CONTAINER_HEIGHT,
      x: 0,
      y: 0,
      toJSON: () => ({}),
    } as DOMRect;
  });
}

afterEach(() => vi.restoreAllMocks());

function Harness({
  onSelect,
  onClear,
  initial = [],
}: {
  onSelect?: (paths: string[]) => void;
  onClear?: () => void;
  initial?: string[];
}) {
  const [selected, setSelected] = useState<string[]>(initial);
  return (
    <SelectionSurface
      currentPaths={selected}
      onSelectPaths={(paths) => {
        setSelected(paths);
        onSelect?.(paths);
      }}
      onClearSelection={() => {
        setSelected([]);
        onClear?.();
      }}
    >
      <div data-testid="scroller" style={{ height: CONTAINER_HEIGHT }}>
        {ROWS.map((path) => (
          <div key={path} data-row data-path={path} data-selected={selected.includes(path)}>
            {path}
          </div>
        ))}
      </div>
    </SelectionSurface>
  );
}

function surface(): HTMLElement {
  return screen.getByTestId('scroller').parentElement as HTMLElement;
}

/** Drag from one viewport point to another. */
function drag(from: [number, number], to: [number, number], options: { ctrlKey?: boolean } = {}) {
  fireEvent.mouseDown(surface(), { button: 0, clientX: from[0], clientY: from[1], ...options });
  fireEvent.mouseMove(window, { clientX: to[0], clientY: to[1] });
}

describe('starting a rectangle', () => {
  it('draws one when empty space is dragged', () => {
    mockBoxes();
    render(<Harness />);
    drag([5, 5], [200, 60]);
    expect(surface().querySelector('[data-rubber-band]')).not.toBeNull();
  });

  it('does not draw one when the press lands on a row', () => {
    // Otherwise a file could never be dragged.
    mockBoxes();
    render(<Harness />);
    const row = screen.getByText('b');
    fireEvent.mouseDown(row, { button: 0, clientX: 10, clientY: 30 });
    fireEvent.mouseMove(window, { clientX: 200, clientY: 90 });
    expect(surface().querySelector('[data-rubber-band]')).toBeNull();
  });

  it('ignores a press that has not passed the threshold', () => {
    mockBoxes();
    render(<Harness />);
    fireEvent.mouseDown(surface(), { button: 0, clientX: 5, clientY: 5 });
    fireEvent.mouseMove(window, { clientX: 6, clientY: 6 });
    expect(surface().querySelector('[data-rubber-band]')).toBeNull();
  });

  it('ignores the secondary button', () => {
    mockBoxes();
    render(<Harness />);
    fireEvent.mouseDown(surface(), { button: 2, clientX: 5, clientY: 5 });
    fireEvent.mouseMove(window, { clientX: 200, clientY: 90 });
    expect(surface().querySelector('[data-rubber-band]')).toBeNull();
  });
});

describe('what the rectangle selects', () => {
  it('selects the rows it covers and leaves the others alone', () => {
    mockBoxes();
    const onSelect = vi.fn();
    render(<Harness onSelect={onSelect} />);

    // From inside row a to inside row c.
    drag([5, 5], [200, 50]);
    expect(onSelect).toHaveBeenLastCalledWith(['a', 'b', 'c']);
  });

  it('selects nothing when the box misses every row', () => {
    mockBoxes();
    const onSelect = vi.fn();
    render(<Harness onSelect={onSelect} />);
    drag([5, 200], [200, 260]);
    expect(onSelect).toHaveBeenLastCalledWith([]);
  });

  it('accounts for the scroll position', () => {
    // Scrolled down 44px, the same viewport band now covers rows c and d.
    mockBoxes(44);
    const onSelect = vi.fn();
    render(<Harness onSelect={onSelect} />);
    Object.defineProperty(surface(), 'scrollTop', { value: 44, writable: true });

    drag([5, 5], [200, 50]);
    expect(onSelect).toHaveBeenLastCalledWith(['c', 'd', 'e']);
  });

  it('adds to the previous selection when ctrl is held', () => {
    mockBoxes();
    const onSelect = vi.fn();
    render(<Harness initial={['e']} onSelect={onSelect} />);
    drag([5, 5], [200, 30], { ctrlKey: true });
    expect(onSelect).toHaveBeenLastCalledWith(['e', 'a', 'b']);
  });

  it('replaces the previous selection without ctrl', () => {
    mockBoxes();
    const onSelect = vi.fn();
    render(<Harness initial={['e']} onSelect={onSelect} />);
    drag([5, 5], [200, 30]);
    expect(onSelect).toHaveBeenLastCalledWith(['a', 'b']);
  });
});

describe('ending the drag', () => {
  it('removes the rectangle on mouse-up', () => {
    mockBoxes();
    render(<Harness />);
    drag([5, 5], [200, 60]);
    expect(surface().querySelector('[data-rubber-band]')).not.toBeNull();

    fireEvent.mouseUp(window);
    expect(surface().querySelector('[data-rubber-band]')).toBeNull();
  });

  it('removes it when the window loses focus mid-drag', () => {
    // Alt-tabbing away must not leave a rectangle painted over the tree.
    mockBoxes();
    render(<Harness />);
    drag([5, 5], [200, 60]);
    fireEvent.blur(window);
    expect(surface().querySelector('[data-rubber-band]')).toBeNull();
  });

  it('clears the selection when empty space is clicked without dragging', () => {
    mockBoxes();
    const onClear = vi.fn();
    render(<Harness initial={['a']} onClear={onClear} />);
    fireEvent.mouseDown(surface(), { button: 0, clientX: 5, clientY: 5 });
    fireEvent.mouseUp(window);
    expect(onClear).toHaveBeenCalledTimes(1);
  });

  it('does not clear on a ctrl-click of empty space', () => {
    mockBoxes();
    const onClear = vi.fn();
    render(<Harness initial={['a']} onClear={onClear} />);
    fireEvent.mouseDown(surface(), { button: 0, clientX: 5, clientY: 5, ctrlKey: true });
    fireEvent.mouseUp(window);
    expect(onClear).not.toHaveBeenCalled();
  });
});

describe('listeners', () => {
  it('takes every window listener back down when unmounted', () => {
    mockBoxes();
    const added = vi.spyOn(window, 'addEventListener');
    const removed = vi.spyOn(window, 'removeEventListener');

    const view = render(<Harness />);
    const addedTypes = added.mock.calls.map(([type]) => type).sort();
    view.unmount();
    const removedTypes = removed.mock.calls.map(([type]) => type).sort();

    for (const type of new Set(addedTypes)) {
      expect(removedTypes).toContain(type);
    }
  });

  it('stops the auto-scroll loop when unmounted mid-drag', () => {
    mockBoxes();
    const cancel = vi.spyOn(window, 'cancelAnimationFrame');
    const view = render(<Harness />);
    drag([5, 5], [200, 60]);
    view.unmount();
    expect(cancel).toHaveBeenCalled();
  });
});

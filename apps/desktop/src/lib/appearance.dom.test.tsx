import { describe, expect, it } from 'vitest';

import { applyAppearance, defaultAppearance } from './appearance';

describe('applying an appearance', () => {
  it('writes every choice onto the element', () => {
    const root = document.createElement('div');

    applyAppearance(root, {
      theme: 'midnight',
      accent: 'violet',
      density: 'compact',
      fontScale: 115,
      motion: 'off',
    });

    expect(root.dataset.theme).toBe('midnight');
    expect(root.dataset.accent).toBe('violet');
    expect(root.dataset.density).toBe('compact');
    expect(root.dataset.motion).toBe('off');
    expect(root.style.getPropertyValue('--font-scale')).toBe('115%');
  });

  /** Switching themes must replace the attribute, not accumulate state: a root
   *  carrying two themes would take whichever block won on source order. */
  it('replaces the previous theme rather than adding to it', () => {
    const root = document.createElement('div');

    applyAppearance(root, { ...defaultAppearance, theme: 'amber' });
    applyAppearance(root, { ...defaultAppearance, theme: 'light' });

    expect(root.dataset.theme).toBe('light');
  });
});

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
    applyAppearance(root, { ...defaultAppearance, theme: 'pure-light' });

    expect(root.dataset.theme).toBe('pure-light');
  });

  /**
   * `auto` has to remove the attribute, not write itself.
   *
   * The accent blocks in the stylesheet exist to override the accent a theme
   * came with. A left-behind `data-accent` would go on doing that for every
   * theme chosen afterwards — the setting would appear to be Auto while a
   * previous choice quietly kept winning.
   */
  it('removes the accent attribute when the theme’s own accent is wanted', () => {
    const root = document.createElement('div');

    applyAppearance(root, { ...defaultAppearance, accent: 'rose' });
    expect(root.dataset.accent).toBe('rose');

    applyAppearance(root, { ...defaultAppearance, accent: 'auto' });
    expect(root.dataset.accent).toBeUndefined();
    expect(root.hasAttribute('data-accent')).toBe(false);
  });

  it('writes an explicit accent over a theme that has its own', () => {
    const root = document.createElement('div');

    applyAppearance(root, { ...defaultAppearance, theme: 'matrix-rain', accent: 'cyan' });

    expect(root.dataset.theme).toBe('matrix-rain');
    expect(root.dataset.accent).toBe('cyan');
  });
});

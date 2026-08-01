import { describe, expect, it } from 'vitest';

import { isTypingTarget, TYPING_SELECTOR } from './shortcuts';

/** Stands in for an element, answering `closest` the way the DOM would. */
function element(matches: boolean) {
  return {
    closest(selector: string) {
      return selector === TYPING_SELECTOR && matches ? {} : null;
    },
  };
}

describe('deciding whether the user is typing', () => {
  it('says yes inside a text box or the editor', () => {
    expect(isTypingTarget(element(true))).toBe(true);
  });

  it('says no for an element outside one', () => {
    expect(isTypingTarget(element(false))).toBe(false);
  });

  it('says no — rather than throwing — for a target that is not an element', () => {
    // A keydown dispatched at the window has one. The earlier version threw
    // here, and because the check ran before every branch it disabled every
    // shortcut in the workspace, not just the ones that consult it.
    expect(isTypingTarget(globalThis)).toBe(false);
    expect(isTypingTarget(null)).toBe(false);
    expect(isTypingTarget(undefined)).toBe(false);
    expect(isTypingTarget('not an element')).toBe(false);
  });

  it('says no for an object that only looks like an element', () => {
    expect(isTypingTarget({ closest: 'not a function' })).toBe(false);
  });
});

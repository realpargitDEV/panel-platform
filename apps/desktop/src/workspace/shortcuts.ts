/**
 * Which keystrokes the workspace is allowed to act on.
 *
 * The distinction that matters is whether the user is typing. Delete inside
 * Monaco deletes a character; the same key with the tree focused deletes a
 * file. Getting that backwards destroys work, so it is a tested function
 * rather than a condition buried in a handler.
 */

/** Anything that swallows ordinary typing. */
export const TYPING_SELECTOR = 'input, textarea, [contenteditable="true"], .monaco-editor';

/** The part of an element this needs. Structural, so a test needs no DOM. */
interface Closest {
  closest(selector: string): unknown;
}

/**
 * Is this event coming from somewhere the user is typing?
 *
 * A target that cannot be asked — a keydown dispatched at the window, which is
 * what synthetic events and some assistive technology produce — is not typing,
 * and must not throw. An earlier version called `closest` unconditionally, and
 * the exception took down every shortcut in the workspace rather than only the
 * two that needed the answer.
 */
export function isTypingTarget(target: unknown): boolean {
  if (target === null || typeof target !== 'object') return false;
  const candidate = target as Partial<Closest>;
  if (typeof candidate.closest !== 'function') return false;
  return candidate.closest(TYPING_SELECTOR) !== null;
}

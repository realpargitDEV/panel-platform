import { defineWorkspace } from 'vitest/config';

// `apps/desktop` is listed explicitly: the editor's tab and dirty-state rules
// are pure functions with their own tests, and a test suite that only covered
// `packages/*` would silently skip them.
export default defineWorkspace(['packages/*', 'apps/desktop']);

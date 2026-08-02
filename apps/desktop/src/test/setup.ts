/**
 * What every DOM test needs before it runs.
 *
 * `@testing-library/jest-dom/vitest` registers the matchers *and* their types
 * against Vitest's `expect`, so `toBeDisabled` and the rest type check as well
 * as run — importing the bare matchers only did the first half.
 */
import '@testing-library/jest-dom/vitest';

import { cleanup } from '@testing-library/react';
import { afterEach } from 'vitest';

// Unmount between tests. React Testing Library only does this automatically
// when globals are on, and a component left mounted keeps its window listeners
// — which is exactly what the rubber-band tests are checking for.
afterEach(() => {
  cleanup();
});

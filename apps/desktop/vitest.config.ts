import react from '@vitejs/plugin-react';
import { defineConfig } from 'vitest/config';

/**
 * The test configuration, kept apart from `vite.config.ts`.
 *
 * Vitest 2 ships Vite 5's types and the application builds on Vite 6, so a
 * single file that imported `defineConfig` from either one would fail to type
 * check against the other. Splitting them costs one file and removes the
 * clash; Vitest reads this in preference to the build config.
 *
 * Tailwind is deliberately absent: no test asserts on a computed style, and
 * running the CSS pipeline for every test file would cost seconds per run.
 */
export default defineConfig({
  plugins: [react()],
  test: {
    // A document only where one is needed. The rules in `selection.ts`,
    // `clipboard.ts` and the rest are pure and run faster without jsdom, and
    // keeping them out of it means a mistake that only works in a browser
    // cannot hide there. `*.dom.test.tsx` opts in.
    environmentMatchGlobs: [['**/*.dom.test.tsx', 'jsdom']],
    setupFiles: ['./src/test/setup.ts'],
  },
});

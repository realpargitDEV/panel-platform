#!/usr/bin/env node
/**
 * Regenerate `apps/desktop/src/themes.generated.css` from the theme catalogue.
 *
 * A script rather than an inline `VAR=1 vitest` in package.json: that syntax is
 * a shell feature, and this repository is developed on Windows, where npm runs
 * scripts through cmd and the assignment would be read as the command name.
 * Spawning with an explicit environment behaves the same everywhere.
 */
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const target = 'apps/desktop/src/lib/themes/generated.test.ts';

const child = spawn(
  process.execPath,
  [fileURLToPath(new URL('../node_modules/vitest/vitest.mjs', import.meta.url)), 'run', target],
  {
    stdio: 'inherit',
    env: { ...process.env, UPDATE_THEME_CSS: '1' },
  },
);

child.on('exit', (code) => process.exit(code ?? 1));

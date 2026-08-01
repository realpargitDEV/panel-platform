/**
 * Which colour a file's icon is drawn in.
 *
 * The shape is always the same document glyph; only the tint changes, which is
 * how Seti — VS Code's default icon theme — reads at 16px. A per-language SVG
 * set would be several hundred paths bundled into a desktop application to
 * decorate a tree, and would still miss the next language.
 *
 * The lookup is by full name first, so `Dockerfile` and `package.json` are
 * recognised as themselves rather than as "no extension" and "some json".
 */

/** The colours are Seti's, so the tree matches the editor's own highlighting. */
const BY_NAME: Record<string, string> = {
  dockerfile: '#519aba',
  'docker-compose.yml': '#519aba',
  makefile: '#6d8086',
  'package.json': '#8dc149',
  'package-lock.json': '#7a7a7a',
  'pnpm-lock.yaml': '#7a7a7a',
  'cargo.toml': '#e37933',
  'cargo.lock': '#7a7a7a',
  'tsconfig.json': '#519aba',
  '.gitignore': '#41535b',
  '.gitattributes': '#41535b',
  '.editorconfig': '#6d8086',
  license: '#cbcb41',
  'readme.md': '#519aba',
};

const BY_EXTENSION: Record<string, string> = {
  ts: '#519aba',
  tsx: '#519aba',
  mts: '#519aba',
  cts: '#519aba',
  js: '#cbcb41',
  jsx: '#cbcb41',
  mjs: '#cbcb41',
  cjs: '#cbcb41',
  json: '#cbcb41',
  jsonc: '#cbcb41',
  rs: '#e37933',
  py: '#519aba',
  rb: '#cc3e44',
  go: '#519aba',
  java: '#cc3e44',
  kt: '#a074c4',
  php: '#a074c4',
  cs: '#a074c4',
  c: '#519aba',
  h: '#a074c4',
  cpp: '#519aba',
  hpp: '#a074c4',
  css: '#519aba',
  scss: '#cc6699',
  less: '#519aba',
  html: '#e37933',
  htm: '#e37933',
  vue: '#8dc149',
  svelte: '#e37933',
  md: '#519aba',
  mdx: '#519aba',
  txt: '#9aa4b2',
  yml: '#cc3e44',
  yaml: '#cc3e44',
  toml: '#6d8086',
  ini: '#6d8086',
  env: '#cbcb41',
  sh: '#8dc149',
  bash: '#8dc149',
  zsh: '#8dc149',
  ps1: '#519aba',
  sql: '#cbcb41',
  png: '#a074c4',
  jpg: '#a074c4',
  jpeg: '#a074c4',
  gif: '#a074c4',
  svg: '#cbcb41',
  webp: '#a074c4',
  ico: '#a074c4',
  zip: '#cc3e44',
  gz: '#cc3e44',
  tar: '#cc3e44',
  lock: '#7a7a7a',
  hbs: '#e37933',
};

/** The tint for anything unrecognised: the same grey the tree text uses. */
export const DEFAULT_FILE_COLOR = '#9aa4b2';

export function fileIconColor(name: string): string {
  const lower = name.toLowerCase();
  const byName = BY_NAME[lower];
  if (byName) return byName;

  const dot = lower.lastIndexOf('.');
  if (dot <= 0) return DEFAULT_FILE_COLOR;
  return BY_EXTENSION[lower.slice(dot + 1)] ?? DEFAULT_FILE_COLOR;
}

/**
 * The language name shown in the status bar.
 *
 * The core already picks a Monaco language id per file and that is what the
 * editor uses; this only turns the id into something worth reading — "TypeScript
 * React" rather than "typescriptreact".
 */
const LANGUAGE_NAMES: Record<string, string> = {
  typescript: 'TypeScript',
  typescriptreact: 'TypeScript React',
  javascript: 'JavaScript',
  javascriptreact: 'JavaScript React',
  json: 'JSON',
  jsonc: 'JSON with Comments',
  rust: 'Rust',
  python: 'Python',
  ruby: 'Ruby',
  go: 'Go',
  java: 'Java',
  kotlin: 'Kotlin',
  php: 'PHP',
  csharp: 'C#',
  c: 'C',
  cpp: 'C++',
  css: 'CSS',
  scss: 'SCSS',
  less: 'Less',
  html: 'HTML',
  markdown: 'Markdown',
  yaml: 'YAML',
  toml: 'TOML',
  ini: 'Ini',
  shell: 'Shell Script',
  powershell: 'PowerShell',
  sql: 'SQL',
  xml: 'XML',
  dockerfile: 'Dockerfile',
  plaintext: 'Plain Text',
};

export function languageName(id: string): string {
  if (!id) return 'Plain Text';
  return LANGUAGE_NAMES[id] ?? id.charAt(0).toUpperCase() + id.slice(1);
}

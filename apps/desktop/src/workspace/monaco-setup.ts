/**
 * Monaco's workers and options.
 *
 * The editor itself is imported as a module and mounted by [`CodeEditor`], so
 * nothing here or there fetches anything: the application is expected to work
 * with no network at all (`docs/offline-mode.md`), and a desktop program that
 * reaches a third-party host to render a text box has no business doing so.
 *
 * `MonacoEnvironment` does the same job for the web workers. Without it Monaco
 * asks for the worker script over the network and, failing that, runs the
 * language service on the UI thread.
 */
import type * as monaco from 'monaco-editor';
// These specifiers go through monaco-editor's `exports` map, which rewrites
// `./*` to `./esm/vs/*.js`. Spelling the old `monaco-editor/esm/vs/...` path
// here resolves to `esm/vs/esm/vs/...` and fails at build time.
import cssWorker from 'monaco-editor/language/css/css.worker?worker';
import htmlWorker from 'monaco-editor/language/html/html.worker?worker';
import jsonWorker from 'monaco-editor/language/json/json.worker?worker';
import tsWorker from 'monaco-editor/language/typescript/ts.worker?worker';
import editorWorker from 'monaco-editor/editor/editor.worker?worker';

declare global {
  interface Window {
    MonacoEnvironment?: monaco.Environment;
  }
}

window.MonacoEnvironment = {
  getWorker(_workerId: string, label: string) {
    switch (label) {
      case 'json':
        return new jsonWorker();
      case 'css':
      case 'scss':
      case 'less':
        return new cssWorker();
      case 'html':
      case 'handlebars':
      case 'razor':
        return new htmlWorker();
      case 'typescript':
      case 'javascript':
        return new tsWorker();
      default:
        return new editorWorker();
    }
  },
};

/**
 * The editor's own options.
 *
 * No language servers and no extensions: the core's `language_for` picks a
 * highlighting mode from the extension and that is the whole of it. Anything
 * more would need a language server per runtime, running on the host — which is
 * the one thing this product does not do.
 */
export const editorOptions: monaco.editor.IStandaloneEditorConstructionOptions = {
  fontSize: 13,
  lineHeight: 19,
  fontFamily:
    "ui-monospace, SFMono-Regular, 'SF Mono', Menlo, Consolas, 'Liberation Mono', monospace",
  // On, and bundled: the minimap is part of Monaco itself, so it costs nothing
  // to show and is the single most recognisable thing about the editor.
  minimap: { enabled: true, renderCharacters: true, maxColumn: 100 },
  scrollBeyondLastLine: false,
  automaticLayout: true,
  tabSize: 2,
  renderWhitespace: 'selection',
  renderLineHighlight: 'line',
  smoothScrolling: true,
  cursorBlinking: 'smooth',
  matchBrackets: 'always',
  bracketPairColorization: { enabled: true },
  guides: { indentation: true, bracketPairs: false },
  // The editor draws its own scrollbar, and it has to look like the panes
  // around it rather than like the platform's.
  scrollbar: { verticalScrollbarSize: 10, horizontalScrollbarSize: 10, useShadows: false },
  padding: { top: 6 },
  // Off: the file on disk is the only source of truth, and a suggestion widget
  // with no language service behind it offers noise rather than help.
  quickSuggestions: false,
  wordBasedSuggestions: 'off',
};

/**
 * The editor's colours, matched to the panes around it.
 *
 * Monaco's own `vs-dark` is grey where this application is navy, and the seam
 * where the editor met the tab strip was visible. Only the surfaces are
 * overridden; the syntax colours are the ones Dark+ ships with.
 */
export const EDITOR_THEME = 'panel-dark';

let themeDefined = false;

export function defineEditorTheme(editor: typeof monaco.editor): void {
  if (themeDefined) return;
  themeDefined = true;
  editor.defineTheme(EDITOR_THEME, {
    base: 'vs-dark',
    inherit: true,
    rules: [],
    colors: {
      'editor.background': '#0d121d',
      'editor.lineHighlightBackground': '#141c2c',
      'editor.lineHighlightBorder': '#00000000',
      'editorLineNumber.foreground': '#4a5670',
      'editorLineNumber.activeForeground': '#a9b3c4',
      'editorGutter.background': '#0d121d',
      'editorIndentGuide.background1': '#1c2537',
      'editorIndentGuide.activeBackground1': '#2f3d59',
      'editorWidget.background': '#12182a',
      'editorWidget.border': '#1a2233',
      'editorSuggestWidget.background': '#12182a',
      'input.background': '#0d121d',
      'input.border': '#1a2233',
      'minimap.background': '#0b101a',
      'scrollbarSlider.background': '#2a355080',
      'scrollbarSlider.hoverBackground': '#3b4a6e80',
      'scrollbarSlider.activeBackground': '#3b4a6ecc',
      'editorOverviewRuler.border': '#00000000',
    },
  });
}

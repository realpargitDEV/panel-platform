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
  fontFamily:
    "ui-monospace, SFMono-Regular, 'SF Mono', Menlo, Consolas, 'Liberation Mono', monospace",
  minimap: { enabled: false },
  scrollBeyondLastLine: false,
  automaticLayout: true,
  tabSize: 2,
  renderWhitespace: 'selection',
  smoothScrolling: true,
  // Off: the file on disk is the only source of truth, and a suggestion widget
  // with no language service behind it offers noise rather than help.
  quickSuggestions: false,
  wordBasedSuggestions: 'off',
};

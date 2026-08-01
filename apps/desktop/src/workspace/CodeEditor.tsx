import * as monaco from 'monaco-editor';
import { useEffect, useRef } from 'react';

import { defineEditorTheme, EDITOR_THEME, editorOptions } from './monaco-setup';

/** Where the caret is, as the status bar reports it. */
export interface CursorPosition {
  line: number;
  column: number;
  /** How many characters are selected, across all selections. Zero when none. */
  selected: number;
}

/** A request to put the caret somewhere: from Problems, or from a search hit. */
export interface RevealRequest {
  line: number;
  column: number;
  /** Changes on every request, so asking for the same line twice works. */
  nonce: number;
}

/**
 * Monaco, mounted directly.
 *
 * `@monaco-editor/react` was tried first and removed. It loads the editor from
 * `cdn.jsdelivr.net` unless told otherwise, and even configured to use the
 * bundled copy it leaves that URL in the bundle — a code path that fetches the
 * text editor over the network, in a product whose offline behaviour is a
 * documented property. Creating the editor by hand is about thirty lines and has
 * no such path.
 *
 * One model per file, recreated when `path` changes, so each file keeps its own
 * undo history and switching tabs does not let an undo in one file rewrite
 * another.
 */
export default function CodeEditor({
  path,
  language,
  value,
  readOnly,
  onChange,
  onSave,
  onCursor,
  reveal,
}: {
  path: string;
  language: string;
  /** Read when the file is opened. Later keystrokes come back through `onChange`. */
  value: string;
  readOnly: boolean;
  onChange: (text: string) => void;
  onSave: () => void;
  onCursor: (position: CursorPosition) => void;
  reveal: RevealRequest | null;
}) {
  const host = useRef<HTMLDivElement | null>(null);
  const instance = useRef<monaco.editor.IStandaloneCodeEditor | null>(null);

  // Held in refs so a new callback identity on every render does not tear the
  // editor down and lose the cursor.
  const changed = useRef(onChange);
  const stored = useRef(onSave);
  const moved = useRef(onCursor);
  changed.current = onChange;
  stored.current = onSave;
  moved.current = onCursor;

  useEffect(() => {
    const container = host.current;
    if (!container) return;

    defineEditorTheme(monaco.editor);

    function report(editor: monaco.editor.IStandaloneCodeEditor) {
      const position = editor.getPosition();
      const model = editor.getModel();
      if (!position || !model) return;
      const selected =
        editor
          .getSelections()
          ?.reduce((total, selection) => total + model.getValueInRange(selection).length, 0) ?? 0;
      moved.current({ line: position.lineNumber, column: position.column, selected });
    }

    // The model is named after the file, so Monaco's own per-file features —
    // markers above all — line up with what the rest of the workspace calls it.
    // A model left behind by a previous mount of the same path is reused rather
    // than duplicated, which Monaco refuses outright.
    const uri = monaco.Uri.parse(`project:/${path}`);
    const model = monaco.editor.getModel(uri) ?? monaco.editor.createModel(value, language, uri);
    const editor = monaco.editor.create(container, {
      ...editorOptions,
      model,
      readOnly,
      theme: EDITOR_THEME,
    });
    instance.current = editor;

    const subscriptions = [
      model.onDidChangeContent(() => changed.current(model.getValue())),
      editor.onDidChangeCursorPosition(() => report(editor)),
      editor.onDidChangeCursorSelection(() => report(editor)),
    ];
    report(editor);

    // Monaco swallows Ctrl+S while it has focus, so the shortcut is registered
    // here as well as on the window.
    editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS, () => stored.current());

    return () => {
      for (const subscription of subscriptions) subscription.dispose();
      editor.dispose();
      model.dispose();
      instance.current = null;
    };
    // `value` is deliberately absent from the dependencies: it is the file's
    // content at open time, and re-running this on every keystroke would
    // recreate the editor under the user's cursor.
  }, [path, language]);

  // A project that starts building mid-edit becomes read-only without the buffer
  // being thrown away.
  useEffect(() => {
    instance.current?.updateOptions({ readOnly });
  }, [readOnly]);

  // Jumping to a problem, or to a line the user asked for.
  useEffect(() => {
    const editor = instance.current;
    if (!editor || !reveal) return;
    editor.revealLineInCenterIfOutsideViewport(reveal.line);
    editor.setPosition({ lineNumber: reveal.line, column: reveal.column });
    editor.focus();
  }, [reveal]);

  return <div ref={host} className="h-full w-full" />;
}

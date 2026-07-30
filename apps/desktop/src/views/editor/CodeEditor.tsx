import * as monaco from 'monaco-editor';
import { useEffect, useRef } from 'react';
import { editorOptions } from './monaco-setup';

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
}: {
  path: string;
  language: string;
  /** Read when the file is opened. Later keystrokes come back through `onChange`. */
  value: string;
  readOnly: boolean;
  onChange: (text: string) => void;
  onSave: () => void;
}) {
  const host = useRef<HTMLDivElement | null>(null);
  const instance = useRef<monaco.editor.IStandaloneCodeEditor | null>(null);

  // Held in refs so a new callback identity on every render does not tear the
  // editor down and lose the cursor.
  const changed = useRef(onChange);
  const saved = useRef(onSave);
  changed.current = onChange;
  saved.current = onSave;

  useEffect(() => {
    const container = host.current;
    if (!container) return;

    const model = monaco.editor.createModel(value, language);
    const editor = monaco.editor.create(container, {
      ...editorOptions,
      model,
      readOnly,
      theme: 'vs-dark',
    });
    instance.current = editor;

    const subscription = model.onDidChangeContent(() => changed.current(model.getValue()));
    // Monaco swallows Ctrl+S while it has focus, so the shortcut is registered
    // here as well as on the window.
    editor.addCommand(monaco.KeyMod.CtrlCmd | monaco.KeyCode.KeyS, () => saved.current());

    return () => {
      subscription.dispose();
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

  return <div ref={host} className="h-full w-full" />;
}

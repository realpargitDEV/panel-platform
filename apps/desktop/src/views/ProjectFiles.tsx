import { useCallback, useEffect, useState } from 'react';
import {
  createProjectFile,
  deleteProjectFile,
  errorMessage,
  listProjectFiles,
  readProjectFile,
  renameProjectFile,
  writeProjectFile,
  type FileEntry,
  type ProjectSummary,
} from '../api';
import CodeEditor from './editor/CodeEditor';
import {
  activeBuffer,
  childPath,
  closeFile,
  edit,
  emptyEditor,
  forgetDeleted,
  isDirty,
  markSaved,
  openFile,
  parentOf,
  renamePath,
  tabLabel,
  toggleExpanded,
  type EditorState,
} from './editor/tabs';

/**
 * Editing a project's files.
 *
 * A file tree, tabs, and Monaco — the editing part of VS Code and none of the
 * rest. Out of scope on purpose: language servers, extensions, a terminal, and
 * any git interface.
 *
 * Every path sent from here is *relative to the project root*. The core takes a
 * project id and a relative string and builds the real path itself, so this view
 * has no way to name a file outside the project even if it tried.
 *
 * The rules about tabs and unsaved changes live in `editor/tabs.ts` as pure
 * functions, and are tested there. What is left here is loading, saving, and
 * rendering.
 */
export default function ProjectFiles({
  project,
  onBack,
}: {
  project: ProjectSummary;
  onBack: () => void;
}) {
  const [listings, setListings] = useState<Record<string, FileEntry[]>>({});
  const [expanded, setExpanded] = useState<string[]>([]);
  const [editor, setEditor] = useState<EditorState>(emptyEditor);
  const [failure, setFailure] = useState<string | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [saving, setSaving] = useState(false);
  /** A tab the user asked to close that has unsaved changes. */
  const [closing, setClosing] = useState<string | null>(null);
  /** Which directory a new file or folder is being named in, and which kind. */
  const [adding, setAdding] = useState<{ directory: string; isFolder: boolean } | null>(null);
  const [newName, setNewName] = useState('');

  const buffer = activeBuffer(editor);
  const dirty = editor.buffers.filter(isDirty).length;

  const loadDirectory = useCallback(
    async (path: string) => {
      try {
        const listing = await listProjectFiles(project.id, path);
        setListings((current) => ({ ...current, [path]: listing.entries }));
        if (listing.truncated) {
          setNotice(`${path || 'This folder'} has more entries than can be shown at once.`);
        }
      } catch (error) {
        setFailure(errorMessage(error));
      }
    },
    [project.id],
  );

  useEffect(() => {
    void loadDirectory('');
  }, [loadDirectory]);

  async function open(entry: FileEntry) {
    setFailure(null);
    try {
      const file = await readProjectFile(project.id, entry.path);
      setEditor((current) => openFile(current, file));
    } catch (error) {
      // A binary or oversized file is refused by the core with a readable
      // reason. Shown as-is rather than opening a garbled buffer.
      setFailure(errorMessage(error));
    }
  }

  const save = useCallback(
    async (path: string) => {
      const target = editor.buffers.find((item) => item.path === path);
      if (!target || target.readOnly) return;

      const written = target.current;
      setSaving(true);
      setFailure(null);
      try {
        await writeProjectFile(project.id, path, written);
        // The text that was written, not what is in the editor now: typing during
        // a save must leave the buffer dirty.
        setEditor((current) => markSaved(current, path, written));
      } catch (error) {
        setFailure(errorMessage(error));
      } finally {
        setSaving(false);
      }
    },
    [editor.buffers, project.id],
  );

  // Ctrl+S / Cmd+S. Registered on the window rather than through Monaco's own
  // keybinding service so it works while the tree has focus too.
  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      if ((event.ctrlKey || event.metaKey) && event.key.toLowerCase() === 's') {
        event.preventDefault();
        if (editor.active !== null) void save(editor.active);
      }
    }
    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [editor.active, save]);

  function requestClose(path: string) {
    const target = editor.buffers.find((item) => item.path === path);
    if (target && isDirty(target)) {
      setClosing(path);
      return;
    }
    setEditor((current) => closeFile(current, path));
  }

  async function submitNew(event: React.FormEvent) {
    event.preventDefault();
    if (!adding || newName.trim().length === 0) return;

    const path = childPath(adding.directory, newName.trim());
    setFailure(null);
    try {
      await createProjectFile(project.id, path, adding.isFolder);
      await loadDirectory(adding.directory);
      setAdding(null);
      setNewName('');
    } catch (error) {
      setFailure(errorMessage(error));
    }
  }

  async function rename(entry: FileEntry) {
    const next = window.prompt('New name', entry.name);
    if (next === null || next.trim().length === 0 || next === entry.name) return;

    setFailure(null);
    try {
      const renamed = await renameProjectFile(project.id, entry.path, next.trim());
      await loadDirectory(parentOf(entry.path));
      // The open tab follows, keeping any unsaved text.
      setEditor((current) => renamePath(current, entry.path, renamed.path));
    } catch (error) {
      setFailure(errorMessage(error));
    }
  }

  async function remove(entry: FileEntry) {
    const isDirectory = entry.kind === 'directory';
    const confirmed = window.confirm(
      isDirectory
        ? `Delete the folder “${entry.name}” and everything in it?`
        : `Delete “${entry.name}”?`,
    );
    if (!confirmed) return;

    setFailure(null);
    try {
      await deleteProjectFile(project.id, entry.path, isDirectory);
      await loadDirectory(parentOf(entry.path));
      setEditor((current) => forgetDeleted(current, entry.path));
    } catch (error) {
      setFailure(errorMessage(error));
    }
  }

  function leave() {
    if (dirty > 0) {
      const confirmed = window.confirm(
        `${dirty} file${dirty === 1 ? '' : 's'} ${dirty === 1 ? 'has' : 'have'} unsaved changes. Leave anyway?`,
      );
      if (!confirmed) return;
    }
    onBack();
  }

  return (
    <div className="flex h-full min-h-0 flex-col px-8 py-7">
      <button
        type="button"
        onClick={leave}
        className="self-start text-sm text-neutral-400 hover:text-neutral-200"
      >
        ← Projects
      </button>

      <div className="mt-4 flex items-baseline gap-3">
        <p className="text-xs font-semibold tracking-wider text-accent uppercase">Files</p>
        <h1 className="text-2xl font-bold tracking-tight">{project.displayName}</h1>
        {buffer?.readOnly && (
          <span className="rounded-full bg-amber-950 px-2.5 py-1 text-xs font-medium text-amber-300">
            read-only while {project.status.toLowerCase()}
          </span>
        )}
      </div>

      {failure && (
        <p className="mt-4 rounded-lg border border-red-900 bg-red-950/60 px-4 py-3 text-sm text-red-200">
          {failure}
        </p>
      )}
      {notice && !failure && (
        <p className="mt-4 rounded-lg border border-edge bg-surface px-4 py-3 text-sm text-neutral-300">
          {notice}
        </p>
      )}

      <div className="mt-5 grid min-h-0 flex-1 gap-4 lg:grid-cols-[260px_1fr]">
        {/* ------------------------------------------------------------ tree */}
        <section className="flex min-h-0 flex-col overflow-hidden rounded-xl border border-edge bg-surface">
          <div className="flex items-center gap-1 border-b border-edge px-3 py-2">
            <span className="flex-1 text-xs font-semibold tracking-wider text-neutral-400 uppercase">
              Explorer
            </span>
            <button
              type="button"
              title="New file in the project root"
              onClick={() => setAdding({ directory: '', isFolder: false })}
              className="rounded px-1.5 py-0.5 text-sm text-neutral-400 hover:bg-white/5 hover:text-neutral-200"
            >
              ＋
            </button>
            <button
              type="button"
              title="New folder in the project root"
              onClick={() => setAdding({ directory: '', isFolder: true })}
              className="rounded px-1.5 py-0.5 text-xs text-neutral-400 hover:bg-white/5 hover:text-neutral-200"
            >
              ＋/
            </button>
            <button
              type="button"
              title="Reload"
              onClick={() => void loadDirectory('')}
              className="rounded px-1.5 py-0.5 text-xs text-neutral-400 hover:bg-white/5 hover:text-neutral-200"
            >
              ⟳
            </button>
          </div>

          <div className="min-h-0 flex-1 overflow-y-auto py-1">
            {adding && adding.directory === '' && (
              <form onSubmit={submitNew} className="px-2 py-1">
                <input
                  autoFocus
                  value={newName}
                  onChange={(event) => setNewName(event.target.value)}
                  onBlur={() => {
                    setAdding(null);
                    setNewName('');
                  }}
                  placeholder={adding.isFolder ? 'folder name' : 'file name'}
                  className="w-full rounded border border-accent bg-black/40 px-2 py-1 font-mono text-xs outline-none select-text"
                />
              </form>
            )}
            <Tree
              directory=""
              depth={0}
              listings={listings}
              expanded={expanded}
              activePath={editor.active}
              onToggle={(path) => {
                setExpanded((current) => toggleExpanded(current, path));
                if (!listings[path]) void loadDirectory(path);
              }}
              onOpen={(entry) => void open(entry)}
              onRename={(entry) => void rename(entry)}
              onDelete={(entry) => void remove(entry)}
            />
          </div>
        </section>

        {/* ---------------------------------------------------------- editor */}
        <section className="flex min-h-0 flex-col overflow-hidden rounded-xl border border-edge bg-surface">
          <div className="flex items-center gap-1 overflow-x-auto border-b border-edge px-2 py-1.5">
            {editor.buffers.length === 0 && (
              <span className="px-2 py-1 text-xs text-neutral-500">No file open</span>
            )}
            {editor.buffers.map((item) => (
              <span
                key={item.path}
                title={item.path}
                className={`flex shrink-0 items-center gap-1.5 rounded-md px-2.5 py-1 text-xs ${
                  editor.active === item.path
                    ? 'bg-accent/15 text-neutral-100'
                    : 'text-neutral-400 hover:bg-white/5'
                }`}
              >
                <button
                  type="button"
                  onClick={() => setEditor((current) => ({ ...current, active: item.path }))}
                >
                  {tabLabel(item.path)}
                  {isDirty(item) && <span className="ml-1 text-accent">●</span>}
                </button>
                <button
                  type="button"
                  title="Close"
                  onClick={() => requestClose(item.path)}
                  className="text-neutral-500 hover:text-neutral-200"
                >
                  ✕
                </button>
              </span>
            ))}

            <span className="flex-1" />
            {buffer && (
              <button
                type="button"
                onClick={() => void save(buffer.path)}
                disabled={saving || buffer.readOnly || !isDirty(buffer)}
                className="shrink-0 rounded-md border border-edge px-2.5 py-1 text-xs disabled:cursor-not-allowed disabled:opacity-40"
              >
                {saving ? 'Saving…' : 'Save'}
              </button>
            )}
          </div>

          {closing !== null && (
            <div className="flex items-center gap-2 border-b border-amber-900/60 bg-amber-950/40 px-3 py-2 text-xs text-amber-200">
              <span className="flex-1">“{tabLabel(closing)}” has unsaved changes.</span>
              <button
                type="button"
                onClick={() => {
                  const path = closing;
                  setClosing(null);
                  void save(path).then(() => setEditor((c) => closeFile(c, path)));
                }}
                className="rounded border border-amber-700 px-2 py-1 hover:bg-amber-900/40"
              >
                Save and close
              </button>
              <button
                type="button"
                onClick={() => {
                  setEditor((current) => closeFile(current, closing));
                  setClosing(null);
                }}
                className="rounded border border-amber-700 px-2 py-1 hover:bg-amber-900/40"
              >
                Discard
              </button>
              <button
                type="button"
                onClick={() => setClosing(null)}
                className="rounded px-2 py-1 hover:bg-amber-900/40"
              >
                Cancel
              </button>
            </div>
          )}

          <div className="min-h-0 flex-1">
            {buffer ? (
              <CodeEditor
                path={buffer.path}
                language={buffer.language}
                value={buffer.current}
                readOnly={buffer.readOnly}
                onChange={(text) => setEditor((current) => edit(current, buffer.path, text))}
                onSave={() => void save(buffer.path)}
              />
            ) : (
              <div className="grid h-full place-items-center px-6 text-center">
                <p className="max-w-sm text-sm leading-relaxed text-neutral-500">
                  Choose a file on the left to edit it. Changes are saved to this project&apos;s
                  folder on this machine; a running project keeps running until you restart it.
                </p>
              </div>
            )}
          </div>

          {buffer && (
            <div className="flex items-center gap-3 border-t border-edge px-3 py-1.5 text-xs text-neutral-500">
              <span className="font-mono">{buffer.path}</span>
              <span>{buffer.language}</span>
              <span className="flex-1" />
              {isDirty(buffer) && <span className="text-accent">unsaved</span>}
              {project.status === 'RUNNING' && !isDirty(buffer) && (
                <span>restart the project to apply saved changes</span>
              )}
            </div>
          )}
        </section>
      </div>
    </div>
  );
}

/**
 * One directory's rows, recursing into the ones that are expanded.
 *
 * A directory with no loaded listing yet renders nothing rather than a spinner
 * per row: the listing arrives in a single call and the delay is imperceptible on
 * a local disk.
 */
function Tree({
  directory,
  depth,
  listings,
  expanded,
  activePath,
  onToggle,
  onOpen,
  onRename,
  onDelete,
}: {
  directory: string;
  depth: number;
  listings: Record<string, FileEntry[]>;
  expanded: string[];
  activePath: string | null;
  onToggle: (path: string) => void;
  onOpen: (entry: FileEntry) => void;
  onRename: (entry: FileEntry) => void;
  onDelete: (entry: FileEntry) => void;
}) {
  const entries = listings[directory];
  if (!entries) return null;

  return (
    <ul>
      {entries.map((entry) => {
        const isDirectory = entry.kind === 'directory';
        const isOpen = expanded.includes(entry.path);

        return (
          <li key={entry.path}>
            <div
              className={`group flex items-center gap-1 pr-2 text-sm ${
                activePath === entry.path ? 'bg-accent/15' : 'hover:bg-white/5'
              }`}
              style={{ paddingLeft: `${depth * 12 + 8}px` }}
            >
              <button
                type="button"
                onClick={() => (isDirectory ? onToggle(entry.path) : onOpen(entry))}
                className="min-w-0 flex-1 py-1 text-left"
              >
                <span className="mr-1.5 text-neutral-500">
                  {isDirectory ? (isOpen ? '▾' : '▸') : entry.kind === 'other' ? '⛔' : ' '}
                </span>
                <span
                  className={`truncate ${
                    entry.kind === 'other' ? 'text-neutral-500 line-through' : ''
                  }`}
                  title={
                    entry.kind === 'other'
                      ? 'A link or special file. It can be deleted but not opened.'
                      : entry.path
                  }
                >
                  {entry.name}
                </span>
              </button>
              <button
                type="button"
                title="Rename"
                onClick={() => onRename(entry)}
                className="hidden px-1 text-xs text-neutral-500 group-hover:block hover:text-neutral-200"
              >
                ✎
              </button>
              <button
                type="button"
                title="Delete"
                onClick={() => onDelete(entry)}
                className="hidden px-1 text-xs text-neutral-500 group-hover:block hover:text-red-300"
              >
                🗑
              </button>
            </div>

            {isDirectory && isOpen && (
              <Tree
                directory={entry.path}
                depth={depth + 1}
                listings={listings}
                expanded={expanded}
                activePath={activePath}
                onToggle={onToggle}
                onOpen={onOpen}
                onRename={onRename}
                onDelete={onDelete}
              />
            )}
          </li>
        );
      })}
    </ul>
  );
}

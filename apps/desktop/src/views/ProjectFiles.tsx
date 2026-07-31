import { useCallback, useEffect, useRef, useState } from 'react';
import {
  appendProjectFileUpload,
  beginProjectFileUpload,
  cancelProjectFileUpload,
  createProjectFile,
  deleteProjectFile,
  errorMessage,
  finishProjectFileUpload,
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

const UPLOAD_CHUNK_BYTES = 512 * 1024;
const UPLOAD_CANCELLED = 'upload-cancelled';

type UploadStatus = 'queued' | 'uploading' | 'success' | 'failed' | 'cancelled';

interface UploadItem {
  id: string;
  uploadId: string;
  file: File;
  path: string;
  uploadedBytes: number;
  sizeBytes: number;
  status: UploadStatus;
  message: string;
}

interface DroppedFile {
  file: File;
  relativePath: string;
}

interface BrowserFileSystemEntry {
  isFile: boolean;
  isDirectory: boolean;
  name: string;
}

interface BrowserFileSystemFileEntry extends BrowserFileSystemEntry {
  file(success: (file: File) => void, failure: (error: DOMException) => void): void;
}

interface BrowserFileSystemDirectoryEntry extends BrowserFileSystemEntry {
  createReader(): BrowserFileSystemDirectoryReader;
}

interface BrowserFileSystemDirectoryReader {
  readEntries(
    success: (entries: BrowserFileSystemEntry[]) => void,
    failure: (error: DOMException) => void,
  ): void;
}

type DataTransferItemWithEntries = DataTransferItem & {
  webkitGetAsEntry?: () => BrowserFileSystemEntry | null;
};

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
  const [selectedDirectory, setSelectedDirectory] = useState('');
  const [draggingOver, setDraggingOver] = useState(false);
  const [uploads, setUploads] = useState<UploadItem[]>([]);
  const cancelledUploads = useRef(new Set<string>());
  const dragDepth = useRef(0);

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

  const updateUpload = useCallback((id: string, patch: Partial<UploadItem>) => {
    setUploads((current) => current.map((item) => (item.id === id ? { ...item, ...patch } : item)));
  }, []);

  const refreshUploadedPath = useCallback(
    async (path: string) => {
      const directories = new Set<string>(['']);
      let directory = parentOf(path);
      directories.add(directory);
      while (directory) {
        directory = parentOf(directory);
        directories.add(directory);
      }
      await Promise.all([...directories].map((item) => loadDirectory(item)));
    },
    [loadDirectory],
  );

  const runUpload = useCallback(
    async (item: UploadItem) => {
      const throwIfCancelled = () => {
        if (cancelledUploads.current.has(item.id)) {
          throw new Error(UPLOAD_CANCELLED);
        }
      };

      updateUpload(item.id, {
        status: 'uploading',
        uploadedBytes: 0,
        message: 'Starting upload...',
      });

      try {
        throwIfCancelled();
        await beginProjectFileUpload(project.id, item.path, item.uploadId, item.sizeBytes);

        let offset = 0;
        while (offset < item.sizeBytes) {
          throwIfCancelled();
          const chunk = item.file.slice(
            offset,
            Math.min(offset + UPLOAD_CHUNK_BYTES, item.sizeBytes),
          );
          const bytes = Array.from(new Uint8Array(await chunk.arrayBuffer()));
          throwIfCancelled();
          offset = await appendProjectFileUpload(
            project.id,
            item.path,
            item.uploadId,
            offset,
            bytes,
          );
          updateUpload(item.id, { uploadedBytes: offset, message: 'Uploading...' });
        }

        throwIfCancelled();
        const uploaded = await finishProjectFileUpload(
          project.id,
          item.path,
          item.uploadId,
          item.sizeBytes,
        );
        updateUpload(item.id, {
          status: 'success',
          uploadedBytes: item.sizeBytes,
          message: 'Uploaded successfully.',
        });
        setNotice(`Uploaded ${uploaded.path}.`);
        await refreshUploadedPath(uploaded.path);
      } catch (error) {
        await cancelProjectFileUpload(project.id, item.path, item.uploadId).catch(() => undefined);
        if (error instanceof Error && error.message === UPLOAD_CANCELLED) {
          updateUpload(item.id, {
            status: 'cancelled',
            message: 'Upload cancelled.',
          });
          return;
        }
        updateUpload(item.id, {
          status: 'failed',
          message: errorMessage(error),
        });
      }
    },
    [project.id, refreshUploadedPath, updateUpload],
  );

  const queueUploads = useCallback(
    (files: DroppedFile[]) => {
      const queued = files
        .map((dropped) => {
          const relativePath = cleanDropPath(dropped.relativePath);
          if (!relativePath) return null;
          const id = createUploadId();
          const item: UploadItem = {
            id,
            uploadId: id,
            file: dropped.file,
            path: childPath(selectedDirectory, relativePath),
            uploadedBytes: 0,
            sizeBytes: dropped.file.size,
            status: 'queued' as const,
            message: 'Waiting to upload...',
          };
          return item;
        })
        .filter((item): item is UploadItem => item !== null);

      if (queued.length === 0) {
        setNotice('No files were found in that drop.');
        return;
      }

      setFailure(null);
      setNotice(
        `Uploading ${queued.length} file${queued.length === 1 ? '' : 's'} to ${displayDirectory(selectedDirectory)}.`,
      );
      setUploads((current) => [...current, ...queued]);
      for (const item of queued) void runUpload(item);
    },
    [runUpload, selectedDirectory],
  );

  const handleDrop = useCallback(
    async (event: React.DragEvent<HTMLElement>) => {
      event.preventDefault();
      dragDepth.current = 0;
      setDraggingOver(false);

      try {
        queueUploads(await collectDroppedFiles(event.dataTransfer));
      } catch (error) {
        setFailure(errorMessage(error));
      }
    },
    [queueUploads],
  );

  function handleDragEnter(event: React.DragEvent<HTMLElement>) {
    event.preventDefault();
    dragDepth.current += 1;
    setDraggingOver(true);
  }

  function handleDragOver(event: React.DragEvent<HTMLElement>) {
    event.preventDefault();
    event.dataTransfer.dropEffect = 'copy';
  }

  function handleDragLeave(event: React.DragEvent<HTMLElement>) {
    event.preventDefault();
    dragDepth.current = Math.max(0, dragDepth.current - 1);
    if (dragDepth.current === 0) setDraggingOver(false);
  }

  const cancelUpload = useCallback(
    (item: UploadItem) => {
      cancelledUploads.current.add(item.id);
      updateUpload(item.id, { status: 'cancelled', message: 'Cancelling upload...' });
      void cancelProjectFileUpload(project.id, item.path, item.uploadId).catch((error) => {
        updateUpload(item.id, { status: 'failed', message: errorMessage(error) });
      });
    },
    [project.id, updateUpload],
  );

  const retryUpload = useCallback(
    (item: UploadItem) => {
      const next = {
        ...item,
        uploadId: createUploadId(),
        uploadedBytes: 0,
        status: 'queued' as const,
        message: 'Waiting to upload...',
      };
      cancelledUploads.current.delete(item.id);
      setUploads((current) => current.map((upload) => (upload.id === item.id ? next : upload)));
      void runUpload(next);
    },
    [runUpload],
  );

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
        <section
          onDragEnter={handleDragEnter}
          onDragLeave={handleDragLeave}
          onDragOver={handleDragOver}
          onDrop={(event) => void handleDrop(event)}
          className={`flex min-h-0 flex-col overflow-hidden rounded-xl border border-edge bg-surface transition ${
            draggingOver ? 'ring-2 ring-accent/80 ring-offset-2 ring-offset-bg' : ''
          }`}
        >
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

          <div
            className={`border-b border-edge px-3 py-2 text-xs ${
              draggingOver ? 'bg-accent/10 text-neutral-100' : 'text-neutral-400'
            }`}
          >
            <div className="mb-1 flex items-center gap-2">
              <span className="font-medium text-neutral-300">Upload target</span>
              <button
                type="button"
                onClick={() => setSelectedDirectory('')}
                className="min-w-0 truncate rounded border border-edge px-2 py-0.5 font-mono text-[11px] text-neutral-200 hover:bg-white/5"
                title="Click to use the project root"
              >
                {displayDirectory(selectedDirectory)}
              </button>
            </div>
            <p>Drop files or folders here. Existing files fail instead of being overwritten.</p>
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
              selectedDirectory={selectedDirectory}
              activePath={editor.active}
              onToggle={(path) => {
                setSelectedDirectory(path);
                setExpanded((current) => toggleExpanded(current, path));
                if (!listings[path]) void loadDirectory(path);
              }}
              onSelectDirectory={setSelectedDirectory}
              onOpen={(entry) => void open(entry)}
              onRename={(entry) => void rename(entry)}
              onDelete={(entry) => void remove(entry)}
            />
          </div>

          {uploads.length > 0 && (
            <UploadList
              uploads={uploads}
              onCancel={cancelUpload}
              onRetry={retryUpload}
              onClearFinished={() =>
                setUploads((current) =>
                  current.filter((item) => item.status === 'queued' || item.status === 'uploading'),
                )
              }
            />
          )}
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
  selectedDirectory,
  activePath,
  onToggle,
  onSelectDirectory,
  onOpen,
  onRename,
  onDelete,
}: {
  directory: string;
  depth: number;
  listings: Record<string, FileEntry[]>;
  expanded: string[];
  selectedDirectory: string;
  activePath: string | null;
  onToggle: (path: string) => void;
  onSelectDirectory: (path: string) => void;
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
        const isSelectedDirectory = isDirectory && selectedDirectory === entry.path;

        return (
          <li key={entry.path}>
            <div
              className={`group flex items-center gap-1 pr-2 text-sm ${
                activePath === entry.path || isSelectedDirectory
                  ? 'bg-accent/15'
                  : 'hover:bg-white/5'
              }`}
              style={{ paddingLeft: `${depth * 12 + 8}px` }}
            >
              <button
                type="button"
                onClick={() => {
                  if (isDirectory) {
                    onSelectDirectory(entry.path);
                    onToggle(entry.path);
                    return;
                  }
                  onOpen(entry);
                }}
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
                selectedDirectory={selectedDirectory}
                activePath={activePath}
                onToggle={onToggle}
                onSelectDirectory={onSelectDirectory}
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

function UploadList({
  uploads,
  onCancel,
  onRetry,
  onClearFinished,
}: {
  uploads: UploadItem[];
  onCancel: (item: UploadItem) => void;
  onRetry: (item: UploadItem) => void;
  onClearFinished: () => void;
}) {
  const hasFinished = uploads.some(
    (item) => item.status === 'success' || item.status === 'failed' || item.status === 'cancelled',
  );

  return (
    <div className="border-t border-edge bg-black/20 px-3 py-2 text-xs">
      <div className="mb-2 flex items-center gap-2">
        <span className="flex-1 font-semibold tracking-wider text-neutral-400 uppercase">
          Uploads
        </span>
        {hasFinished && (
          <button
            type="button"
            onClick={onClearFinished}
            className="rounded px-2 py-0.5 text-neutral-500 hover:bg-white/5 hover:text-neutral-200"
          >
            Clear finished
          </button>
        )}
      </div>
      <div className="max-h-44 space-y-2 overflow-y-auto pr-1">
        {uploads.map((item) => {
          const percent = uploadPercent(item);
          const canCancel = item.status === 'queued' || item.status === 'uploading';
          const canRetry = item.status === 'failed' || item.status === 'cancelled';

          return (
            <div key={item.id} className="rounded-lg border border-edge bg-surface/80 p-2">
              <div className="flex items-start gap-2">
                <div className="min-w-0 flex-1">
                  <p className="truncate font-mono text-[11px] text-neutral-200" title={item.path}>
                    {item.path}
                  </p>
                  <p className={`mt-0.5 ${uploadTextClass(item.status)}`}>{item.message}</p>
                </div>
                {canCancel && (
                  <button
                    type="button"
                    onClick={() => onCancel(item)}
                    className="rounded px-2 py-0.5 text-neutral-400 hover:bg-white/5 hover:text-neutral-100"
                  >
                    Cancel
                  </button>
                )}
                {canRetry && (
                  <button
                    type="button"
                    onClick={() => onRetry(item)}
                    className="rounded px-2 py-0.5 text-neutral-400 hover:bg-white/5 hover:text-neutral-100"
                  >
                    Retry
                  </button>
                )}
              </div>
              <div className="mt-2 h-1.5 overflow-hidden rounded-full bg-black/50">
                <div
                  className={`h-full transition-all ${uploadBarClass(item.status)}`}
                  style={{ width: `${percent}%` }}
                />
              </div>
              <div className="mt-1 flex items-center gap-2 text-[11px] text-neutral-500">
                <span>{uploadStatusLabel(item.status)}</span>
                <span className="flex-1" />
                <span>
                  {formatUploadBytes(item.uploadedBytes)} / {formatUploadBytes(item.sizeBytes)}
                </span>
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}

async function collectDroppedFiles(dataTransfer: DataTransfer): Promise<DroppedFile[]> {
  const dropped: DroppedFile[] = [];
  const items = Array.from(dataTransfer.items);

  if (items.length > 0) {
    for (const item of items) {
      if (item.kind !== 'file') continue;
      const entry = (item as DataTransferItemWithEntries).webkitGetAsEntry?.();
      if (entry) {
        dropped.push(...(await collectEntryFiles(entry, '')));
        continue;
      }

      const file = item.getAsFile();
      if (file) dropped.push({ file, relativePath: file.webkitRelativePath || file.name });
    }
    if (dropped.length > 0) return dropped;
  }

  return Array.from(dataTransfer.files).map((file) => ({
    file,
    relativePath: file.webkitRelativePath || file.name,
  }));
}

async function collectEntryFiles(
  entry: BrowserFileSystemEntry,
  prefix: string,
): Promise<DroppedFile[]> {
  const relativePath = childPath(prefix, entry.name);
  if (entry.isFile) {
    const file = await fileFromEntry(entry as BrowserFileSystemFileEntry);
    return [{ file, relativePath }];
  }
  if (!entry.isDirectory) return [];

  const childEntries = await readAllEntries(entry as BrowserFileSystemDirectoryEntry);
  const nested = await Promise.all(
    childEntries.map((child) => collectEntryFiles(child, relativePath)),
  );
  return nested.flat();
}

function fileFromEntry(entry: BrowserFileSystemFileEntry): Promise<File> {
  return new Promise((resolve, reject) => entry.file(resolve, reject));
}

async function readAllEntries(
  entry: BrowserFileSystemDirectoryEntry,
): Promise<BrowserFileSystemEntry[]> {
  const reader = entry.createReader();
  const entries: BrowserFileSystemEntry[] = [];

  while (true) {
    const batch = await new Promise<BrowserFileSystemEntry[]>((resolve, reject) =>
      reader.readEntries(resolve, reject),
    );
    if (batch.length === 0) return entries;
    entries.push(...batch);
  }
}

function cleanDropPath(path: string): string {
  return path
    .replace(/\\/g, '/')
    .split('/')
    .map((part) => part.trim())
    .filter((part) => part.length > 0 && part !== '.')
    .join('/');
}

function createUploadId(): string {
  if (typeof crypto.randomUUID === 'function') return crypto.randomUUID();
  return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
}

function displayDirectory(path: string): string {
  return path ? `/${path}` : '/ (project root)';
}

function uploadPercent(item: UploadItem): number {
  if (item.status === 'success') return 100;
  if (item.sizeBytes === 0) return item.status === 'uploading' ? 50 : 100;
  return Math.max(0, Math.min(100, Math.round((item.uploadedBytes / item.sizeBytes) * 100)));
}

function uploadStatusLabel(status: UploadStatus): string {
  switch (status) {
    case 'queued':
      return 'Queued';
    case 'uploading':
      return 'Uploading';
    case 'success':
      return 'Complete';
    case 'failed':
      return 'Failed';
    case 'cancelled':
      return 'Cancelled';
  }
}

function uploadTextClass(status: UploadStatus): string {
  switch (status) {
    case 'success':
      return 'text-emerald-300';
    case 'failed':
      return 'text-red-300';
    case 'cancelled':
      return 'text-amber-300';
    default:
      return 'text-neutral-400';
  }
}

function uploadBarClass(status: UploadStatus): string {
  switch (status) {
    case 'success':
      return 'bg-emerald-400';
    case 'failed':
      return 'bg-red-400';
    case 'cancelled':
      return 'bg-amber-400';
    default:
      return 'bg-accent';
  }
}

function formatUploadBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
}

import { getCurrentWebviewWindow } from '@tauri-apps/api/webviewWindow';
import * as monaco from 'monaco-editor';
import { useCallback, useEffect, useMemo, useRef, useState } from 'react';

import {
  appendProjectFileUpload,
  appSettings,
  beginProjectFileUpload,
  cancelProjectFileImport,
  cancelProjectFileUpload,
  copyProjectFile,
  createProjectFile,
  deleteProjectFile,
  errorMessage,
  finishProjectFileUpload,
  importProjectFiles,
  killProject,
  listProjectFiles,
  moveProjectFile,
  onFileImportProgress,
  projectRootPath,
  readProjectFile,
  renameProjectFile,
  restartProject,
  revealProjectPath,
  searchProjectFiles,
  startProject,
  stopProject,
  writeProjectFile,
  type AppSettings,
  type FileEntry,
  type ProjectSummary,
  type SystemStatus,
} from '../api';
import { buttonLabel, canStart } from '../update';
import { updateStore, useUpdate } from '../useUpdate';
import ActivityBar from './ActivityBar';
import BottomPanel from './BottomPanel';
import CodeEditor, { type CursorPosition, type RevealRequest } from './CodeEditor';
import CommandPalette, { type PaletteMode } from './CommandPalette';
import type { Command } from './commands';
import { conflictingPaths, resolveConflicts, type ConflictChoice } from './conflicts';
import ContextMenu, { type MenuEntry, type MenuPosition } from './ContextMenu';
import { ConfirmDialog, ConflictDialog } from './Dialogs';
import {
  cleanDropPath,
  collectDroppedItems,
  directoryPathsForDrop,
  duplicateDroppedFilePath,
  isInsideDropZone,
  shouldImportBrowserDrop,
  type DropPoint,
  type DroppedItems,
} from './dropImport';
import EditorTabs from './EditorTabs';
import EmptyEditorState, { type WelcomeAction } from './EmptyEditorState';
import ExplorerPanel from './ExplorerPanel';
import type { InlineEdit, TreeState } from './FileTree';
import { languageName } from './fileIcons';
import Icon from './Icon';
import {
  clampPanelHeight,
  clampSidebarWidth,
  loadLayout,
  saveLayout,
  type ActivityView,
  type Layout,
  type PanelTab,
} from './layout';
import { appendLine, linesForChannel, type OutputChannel, type OutputLine } from './output';
import { LogsPanel, OutputPanel, ProblemsPanel, TerminalPanel, type RunAction } from './panels';
import { countProblems, useProblems, type Problem } from './problems';
import ResizeHandle from './ResizeHandle';
import { isTypingTarget } from './shortcuts';
import {
  AccountPanel,
  ExtensionsPanel,
  RunPanel,
  SearchPanel,
  SourceControlPanel,
  useFileSearch,
} from './SidebarSections';
import StatusBar from './StatusBar';
import {
  activeBuffer,
  bufferFor,
  childPath,
  closeAll,
  closeFile,
  closeOthers,
  closeToRight,
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
} from './tabs';
import TitleBar from './TitleBar';
import type { Menu } from './MenuBar';
import {
  createUploadId,
  displayDirectory,
  nativeImportLabel,
  UPLOAD_CANCELLED,
  UPLOAD_CHUNK_BYTES,
  type BrowserUploadItem,
  type NativeImportItem,
  type UploadItem,
  type UploadPatch,
} from './uploads';

/**
 * One project, as a workspace.
 *
 * This is the whole editor: the menu bar, the activity bar, the explorer, the
 * tabs, Monaco, the panel and the status bar. It replaced two screens — a file
 * browser and a separate console — because they were two halves of the same
 * job and switching between them lost the editor's state.
 *
 * The file it is not: a component library. Everything visual is a component in
 * this folder, and everything with a rule in it that can be got wrong is a
 * tested pure function in `tabs.ts`, `layout.ts`, `conflicts.ts` or
 * `commands.ts`. What is left here is the wiring — the API calls, and the state
 * that several panes have to agree on.
 *
 * Every path sent from here is *relative to the project root*. The core takes a
 * project id and a relative string and builds the real path itself, so this
 * view has no way to name a file outside the project even if it tried.
 */
export default function ProjectWorkspace({
  project,
  status,
  dockerAvailable,
  onRefreshProjects,
  onLeave,
  onOpenSettings,
}: {
  project: ProjectSummary;
  status: SystemStatus | null;
  dockerAvailable: boolean;
  onRefreshProjects: () => Promise<void>;
  onLeave: () => void;
  onOpenSettings: () => void;
}) {
  // ------------------------------------------------------------------ state
  const [listings, setListings] = useState<Record<string, FileEntry[]>>({});
  const [expanded, setExpanded] = useState<string[]>([]);
  const [editor, setEditor] = useState<EditorState>(emptyEditor);
  const [selected, setSelected] = useState<string | null>(null);
  const [targetDirectory, setTargetDirectory] = useState('');
  const [editing, setEditing] = useState<InlineEdit | null>(null);
  const [saving, setSaving] = useState(false);
  const [cursor, setCursor] = useState<CursorPosition | null>(null);
  const [reveal, setReveal] = useState<RevealRequest | null>(null);

  const [layout, setLayout] = useState<Layout>(() =>
    loadLayout(typeof window === 'undefined' ? undefined : window.localStorage),
  );
  const [palette, setPalette] = useState<PaletteMode | null>(null);
  const [menu, setMenu] = useState<{ entries: MenuEntry[]; position: MenuPosition } | null>(null);
  const [confirm, setConfirm] = useState<{
    title: string;
    detail?: string;
    confirmLabel: string;
    danger?: boolean;
    onConfirm: () => void;
  } | null>(null);

  const [uploads, setUploads] = useState<UploadItem[]>([]);
  const [draggingOver, setDraggingOver] = useState(false);
  const [conflict, setConflict] = useState<{
    paths: string[];
    directory: string;
    resolve: (choice: ConflictChoice | null) => void;
  } | null>(null);

  const [output, setOutput] = useState<OutputLine[]>([]);
  const [outputChannel, setOutputChannel] = useState<OutputChannel | 'all'>('all');
  const [busyAction, setBusyAction] = useState<RunAction | null>(null);
  const [settings, setSettings] = useState<AppSettings | null>(null);
  const [projectRoot, setProjectRoot] = useState<string | null>(null);

  const [searchQuery, setSearchQuery] = useState('');
  const [quickOpenTerm, setQuickOpenTerm] = useState('');

  /** Where Back and Forward go: the files opened, in order. */
  const [history, setHistory] = useState<{ paths: string[]; index: number }>({
    paths: [],
    index: -1,
  });

  const cancelledUploads = useRef(new Set<string>());
  const dragDepth = useRef(0);
  const outputId = useRef(0);
  /**
   * Whether Tauri's OS-level drag/drop listener is live. While it is, it owns
   * every drop and the HTML5 handlers below do nothing — see
   * `shouldImportBrowserDrop`.
   */
  const nativeListenerReady = useRef(false);
  const explorer = useRef<HTMLDivElement | null>(null);

  const problems = useProblems();
  const { errors, warnings } = countProblems(problems);
  const update = useUpdate();

  const buffer = activeBuffer(editor);
  const dirtyCount = editor.buffers.filter(isDirty).length;

  // ----------------------------------------------------------------- output
  const say = useCallback(
    (channel: OutputChannel, text: string, level: OutputLine['level'] = 'info') => {
      outputId.current += 1;
      setOutput((current) =>
        appendLine(current, { id: outputId.current, at: new Date(), channel, level, text }),
      );
    },
    [],
  );

  const report = useCallback(
    (channel: OutputChannel, error: unknown) => {
      say(channel, errorMessage(error), 'error');
    },
    [say],
  );

  // ------------------------------------------------------------- the layout
  const patchLayout = useCallback((patch: Partial<Layout>) => {
    setLayout((current) => {
      const next = { ...current, ...patch };
      saveLayout(typeof window === 'undefined' ? undefined : window.localStorage, next);
      return next;
    });
  }, []);

  const showPanel = useCallback(
    (tab: PanelTab) => patchLayout({ panelVisible: true, panelTab: tab }),
    [patchLayout],
  );

  const showSidebar = useCallback(
    (view: ActivityView) => patchLayout({ sidebarVisible: true, activityView: view }),
    [patchLayout],
  );

  // The window shrinking below a stored size has to move the panels, or the
  // editor is squeezed out of existence by a layout from a bigger monitor.
  useEffect(() => {
    function onResize() {
      setLayout((current) => ({
        ...current,
        sidebarWidth: clampSidebarWidth(current.sidebarWidth, window.innerWidth),
        panelHeight: clampPanelHeight(current.panelHeight, window.innerHeight),
      }));
    }
    onResize();
    window.addEventListener('resize', onResize);
    return () => window.removeEventListener('resize', onResize);
  }, []);

  // ----------------------------------------------------------- loading files
  const loadDirectory = useCallback(
    async (path: string) => {
      try {
        const listing = await listProjectFiles(project.id, path);
        setListings((current) => ({ ...current, [path]: listing.entries }));
        if (listing.truncated) {
          say(
            'Files',
            `${displayDirectory(path)} has more entries than the core will return at once; some are not shown.`,
            'warn',
          );
        }
      } catch (error) {
        report('Files', error);
      }
    },
    [project.id, report, say],
  );

  useEffect(() => {
    void loadDirectory('');
  }, [loadDirectory]);

  useEffect(() => {
    projectRootPath(project.id)
      .then(setProjectRoot)
      .catch(() => setProjectRoot(null));
    appSettings()
      .then(setSettings)
      .catch(() => setSettings(null));
  }, [project.id]);

  /** Reload one folder and every folder above it. */
  const refreshBranch = useCallback(
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

  /** Which paths are known to exist under a folder, for the conflict check. */
  const pathsIn = useCallback(
    async (directory: string): Promise<string[]> => {
      try {
        const listing = await listProjectFiles(project.id, directory);
        return listing.entries.map((entry) => entry.path);
      } catch {
        // A folder that cannot be listed cannot be checked; the core still
        // refuses to overwrite, so the upload fails safely rather than silently.
        return [];
      }
    },
    [project.id],
  );

  // ---------------------------------------------------------------- opening
  const openPath = useCallback(
    async (path: string) => {
      try {
        const file = await readProjectFile(project.id, path);
        setEditor((current) => openFile(current, file));
        setSelected(path);
        setTargetDirectory(parentOf(path));
        setHistory((current) => {
          if (current.paths[current.index] === path) return current;
          const paths = [...current.paths.slice(0, current.index + 1), path];
          return { paths, index: paths.length - 1 };
        });
      } catch (error) {
        // A binary or oversized file is refused by the core with a readable
        // reason. Shown as-is rather than opening a garbled buffer.
        report('Files', error);
      }
    },
    [project.id, report],
  );

  const save = useCallback(
    async (path: string) => {
      const target = bufferFor(editor, path);
      if (!target || target.readOnly) return;

      const written = target.current;
      setSaving(true);
      try {
        await writeProjectFile(project.id, path, written);
        // The text that was written, not what is in the editor now: typing
        // during a save must leave the buffer dirty.
        setEditor((current) => markSaved(current, path, written));
        say('Files', `Saved ${path}.`);
      } catch (error) {
        report('Files', error);
      } finally {
        setSaving(false);
      }
    },
    [editor, project.id, report, say],
  );

  const requestClose = useCallback(
    (path: string) => {
      const target = bufferFor(editor, path);
      if (target && isDirty(target)) {
        setConfirm({
          title: `Do you want to save the changes you made to ${tabLabel(path)}?`,
          detail: 'Your changes will be lost if you do not save them.',
          confirmLabel: 'Save',
          onConfirm: () => {
            setConfirm(null);
            void save(path).then(() => setEditor((current) => closeFile(current, path)));
          },
        });
        return;
      }
      setEditor((current) => closeFile(current, path));
    },
    [editor, save],
  );

  // ------------------------------------------------------------- file edits
  const beginCreate = useCallback(
    (directory: string, isFolder: boolean) => {
      showSidebar('explorer');
      setExpanded((current) => (current.includes(directory) ? current : [...current, directory]));
      if (directory && !listings[directory]) void loadDirectory(directory);
      setEditing({
        kind: isFolder ? 'create-folder' : 'create-file',
        path: directory,
        initialValue: '',
      });
    },
    [listings, loadDirectory, showSidebar],
  );

  const beginRename = useCallback((entry: { path: string; name: string }) => {
    setEditing({ kind: 'rename', path: entry.path, initialValue: entry.name });
  }, []);

  const submitEdit = useCallback(
    async (value: string) => {
      const request = editing;
      setEditing(null);
      if (!request) return;

      try {
        if (request.kind === 'rename') {
          const renamed = await renameProjectFile(project.id, request.path, value);
          await loadDirectory(parentOf(request.path));
          // The open tab follows, keeping any unsaved text.
          setEditor((current) => renamePath(current, request.path, renamed.path));
          setSelected(renamed.path);
          say('Files', `Renamed ${request.path} to ${renamed.path}.`);
          return;
        }

        const isFolder = request.kind === 'create-folder';
        const path = childPath(request.path, value);
        const created = await createProjectFile(project.id, path, isFolder);
        await loadDirectory(request.path);
        say('Files', `Created ${created.path}.`);
        if (isFolder) {
          setExpanded((current) => (current.includes(path) ? current : [...current, path]));
          setTargetDirectory(path);
          setSelected(path);
        } else {
          await openPath(created.path);
        }
      } catch (error) {
        report('Files', error);
      }
    },
    [editing, loadDirectory, openPath, project.id, report, say],
  );

  const remove = useCallback(
    (entry: FileEntry) => {
      const isDirectory = entry.kind === 'directory';
      setConfirm({
        title: isDirectory
          ? `Are you sure you want to delete “${entry.name}” and everything inside it?`
          : `Are you sure you want to delete “${entry.name}”?`,
        detail: 'This cannot be undone.',
        confirmLabel: 'Delete',
        danger: true,
        onConfirm: () => {
          setConfirm(null);
          void (async () => {
            try {
              await deleteProjectFile(project.id, entry.path, isDirectory);
              await loadDirectory(parentOf(entry.path));
              setEditor((current) => forgetDeleted(current, entry.path));
              setSelected((current) => (current === entry.path ? null : current));
              say('Files', `Deleted ${entry.path}.`);
            } catch (error) {
              report('Files', error);
            }
          })();
        },
      });
    },
    [loadDirectory, project.id, report, say],
  );

  const duplicate = useCallback(
    async (entry: FileEntry) => {
      const directory = parentOf(entry.path);
      const existing = await pathsIn(directory);
      const dot = entry.name.lastIndexOf('.');
      const stem = dot > 0 ? entry.name.slice(0, dot) : entry.name;
      const extension = dot > 0 ? entry.name.slice(dot) : '';

      let candidate = childPath(directory, `${stem} copy${extension}`);
      for (let counter = 2; existing.includes(candidate); counter += 1) {
        candidate = childPath(directory, `${stem} copy ${counter}${extension}`);
      }

      try {
        const copied = await copyProjectFile(project.id, entry.path, candidate);
        await loadDirectory(directory);
        say('Files', `Copied ${entry.path} to ${copied.path}.`);
      } catch (error) {
        report('Files', error);
      }
    },
    [loadDirectory, pathsIn, project.id, report, say],
  );

  const move = useCallback(
    async (from: string, toDirectory: string) => {
      const name = from.slice(from.lastIndexOf('/') + 1);
      const to = childPath(toDirectory, name);
      if (to === from) return;
      // Dropping a folder into itself, or into its own child, would move the
      // tree under its own feet. The core refuses it too; catching it here
      // keeps the error out of the panel for something the user cannot mean.
      if (toDirectory === from || toDirectory.startsWith(`${from}/`)) {
        say('Files', `${from} cannot be moved inside itself.`, 'warn');
        return;
      }

      try {
        const moved = await moveProjectFile(project.id, from, to);
        await Promise.all([loadDirectory(parentOf(from)), loadDirectory(toDirectory)]);
        setEditor((current) => renamePath(current, from, moved.path));
        setSelected(moved.path);
        say('Files', `Moved ${from} to ${moved.path}.`);
      } catch (error) {
        report('Files', error);
      }
    },
    [loadDirectory, project.id, report, say],
  );

  const copyToClipboard = useCallback(
    (text: string, description: string) => {
      navigator.clipboard
        .writeText(text)
        .then(() => say('Workspace', `Copied ${description}.`))
        .catch((error: unknown) => report('Workspace', error));
    },
    [report, say],
  );

  const reveal_ = useCallback(
    async (path: string) => {
      try {
        await revealProjectPath(project.id, path);
      } catch (error) {
        report('Files', error);
      }
    },
    [project.id, report],
  );

  // ---------------------------------------------------------------- uploads
  const updateUpload = useCallback((id: string, patch: UploadPatch) => {
    setUploads((current) => current.map((item) => (item.id === id ? { ...item, ...patch } : item)));
  }, []);

  const runUpload = useCallback(
    async (item: BrowserUploadItem) => {
      const throwIfCancelled = () => {
        if (cancelledUploads.current.has(item.id)) throw new Error(UPLOAD_CANCELLED);
      };

      updateUpload(item.id, { status: 'uploading', uploadedBytes: 0, message: 'Starting upload…' });

      try {
        throwIfCancelled();
        // A replacement deletes first: the core will not overwrite, and the
        // user has already been asked.
        if (item.replaces) {
          await deleteProjectFile(project.id, item.path, false);
        }

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
          updateUpload(item.id, { uploadedBytes: offset, message: 'Uploading…' });
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
          message: 'Uploaded.',
        });
        say('Transfers', `Uploaded ${uploaded.path}.`);
        await refreshBranch(uploaded.path);
      } catch (error) {
        await cancelProjectFileUpload(project.id, item.path, item.uploadId).catch(() => undefined);
        if (error instanceof Error && error.message === UPLOAD_CANCELLED) {
          updateUpload(item.id, { status: 'cancelled', message: 'Cancelled.' });
          return;
        }
        const message = errorMessage(error);
        updateUpload(item.id, { status: 'failed', message });
        say('Transfers', `${item.path}: ${message}`, 'error');
      }
    },
    [project.id, refreshBranch, say, updateUpload],
  );

  const runNativeImport = useCallback(
    async (item: NativeImportItem) => {
      updateUpload(item.id, {
        status: 'uploading',
        uploadedBytes: 0,
        sizeBytes: 0,
        copiedFiles: 0,
        totalFiles: 0,
        message: 'Preparing import…',
      });

      try {
        const imported = await importProjectFiles(
          project.id,
          item.targetDirectory,
          item.sourcePaths,
          item.uploadId,
        );
        updateUpload(item.id, { status: 'success', message: 'Imported.' });
        say(
          'Transfers',
          `Imported ${imported.length} item${imported.length === 1 ? '' : 's'} into ${displayDirectory(item.targetDirectory)}.`,
        );
        await Promise.all(imported.map((entry) => refreshBranch(entry.path)));
        await loadDirectory(item.targetDirectory);
      } catch (error) {
        const message = errorMessage(error);
        if (cancelledUploads.current.has(item.id) || message.toLowerCase().includes('cancelled')) {
          updateUpload(item.id, { status: 'cancelled', message: 'Cancelled.' });
          return;
        }
        updateUpload(item.id, { status: 'failed', message });
        say('Transfers', message, 'error');
      }
    },
    [loadDirectory, project.id, refreshBranch, say, updateUpload],
  );

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let active = true;

    void onFileImportProgress((progress) => {
      if (progress.projectId !== project.id) return;
      updateUpload(progress.importId, {
        uploadedBytes: progress.copiedBytes,
        sizeBytes: progress.totalBytes,
        copiedFiles: progress.copiedFiles,
        totalFiles: progress.totalFiles,
        message: progress.currentPath ? `Importing ${progress.currentPath}…` : 'Preparing import…',
      });
    })
      .then((next) => {
        if (active) unlisten = next;
        else next();
      })
      .catch((error: unknown) => report('Transfers', error));

    return () => {
      active = false;
      unlisten?.();
    };
  }, [project.id, report, updateUpload]);

  /** Ask about existing files, once for the whole drop. */
  const chooseConflictAction = useCallback(
    (paths: string[], directory: string): Promise<ConflictChoice | null> =>
      new Promise((resolve) => {
        setConflict({
          paths,
          directory,
          resolve: (choice) => {
            setConflict(null);
            resolve(choice);
          },
        });
      }),
    [],
  );

  const queueBrowserDrop = useCallback(
    (dropped: DroppedItems) => {
      const duplicatePath = duplicateDroppedFilePath(dropped.files);
      if (duplicatePath) {
        say(
          'Transfers',
          `The drop contains two files called "${duplicatePath}". Rename one and try again.`,
          'error',
        );
        showPanel('output');
        return;
      }

      const directories = directoryPathsForDrop(dropped.files, dropped.directories);
      const candidates = dropped.files
        .map((entry) => {
          const relativePath = cleanDropPath(entry.relativePath);
          if (!relativePath) return null;
          return { file: entry.file, path: childPath(targetDirectory, relativePath) };
        })
        .filter((entry): entry is { file: File; path: string } => entry !== null);

      if (candidates.length === 0 && directories.length === 0) {
        say('Transfers', 'No files were found in that drop.', 'warn');
        return;
      }

      void (async () => {
        try {
          // Only the files landing directly in the target can clash with
          // something already listed; a nested path lands in a folder this
          // drop is creating.
          const existing = await pathsIn(targetDirectory);
          const clashes = conflictingPaths(
            existing,
            candidates.map((entry) => entry.path),
          );

          let choice: ConflictChoice = 'rename';
          if (clashes.length > 0) {
            const chosen = await chooseConflictAction(clashes, targetDirectory);
            if (chosen === null) {
              say('Transfers', 'Import cancelled.', 'warn');
              return;
            }
            choice = chosen;
          }

          const { uploads: resolved, skipped } = resolveConflicts(existing, candidates, choice);
          if (skipped.length > 0) {
            say('Transfers', `Skipped ${skipped.length} file(s) that already exist.`, 'warn');
          }

          for (const directory of directories) {
            await createProjectFile(project.id, childPath(targetDirectory, directory), true).catch(
              () => undefined,
            );
          }

          const queued: BrowserUploadItem[] = resolved.map((entry) => {
            const id = createUploadId();
            return {
              kind: 'browser',
              id,
              uploadId: id,
              file: entry.item.file,
              path: entry.path,
              replaces: entry.replaces,
              uploadedBytes: 0,
              sizeBytes: entry.item.file.size,
              status: 'queued',
              message: 'Waiting to upload…',
            };
          });

          if (queued.length > 0) {
            setUploads((current) => [...current, ...queued]);
            for (const item of queued) void runUpload(item);
          } else if (directories.length > 0) {
            say(
              'Transfers',
              `Created ${directories.length} folder${directories.length === 1 ? '' : 's'} in ${displayDirectory(targetDirectory)}.`,
            );
          }
          await loadDirectory(targetDirectory);
        } catch (error) {
          report('Transfers', error);
        }
      })();
    },
    [
      chooseConflictAction,
      loadDirectory,
      pathsIn,
      project.id,
      report,
      runUpload,
      say,
      showPanel,
      targetDirectory,
    ],
  );

  const queueNativeImport = useCallback(
    (paths: string[]) => {
      const sourcePaths = [...new Set(paths.filter((path) => path.trim().length > 0))];
      if (sourcePaths.length === 0) {
        say('Transfers', 'No files or folders were found in that drop.', 'warn');
        return;
      }

      const id = createUploadId();
      const item: NativeImportItem = {
        kind: 'native',
        id,
        uploadId: id,
        sourcePaths,
        targetDirectory,
        path: nativeImportLabel(sourcePaths, targetDirectory),
        uploadedBytes: 0,
        sizeBytes: 0,
        copiedFiles: 0,
        totalFiles: 0,
        status: 'queued',
        message: 'Waiting to import…',
      };

      say(
        'Transfers',
        `Importing ${sourcePaths.length} item${sourcePaths.length === 1 ? '' : 's'} into ${displayDirectory(targetDirectory)}.`,
      );
      setUploads((current) => [...current, item]);
      void runNativeImport(item);
    },
    [runNativeImport, say, targetDirectory],
  );

  const cancelUpload = useCallback(
    (item: UploadItem) => {
      cancelledUploads.current.add(item.id);
      updateUpload(item.id, { status: 'cancelled', message: 'Cancelling…' });
      const cancel =
        item.kind === 'native'
          ? cancelProjectFileImport(item.uploadId)
          : cancelProjectFileUpload(project.id, item.path, item.uploadId);
      void cancel.catch((error: unknown) => {
        updateUpload(item.id, { status: 'failed', message: errorMessage(error) });
      });
    },
    [project.id, updateUpload],
  );

  const retryUpload = useCallback(
    (item: UploadItem) => {
      const uploadId = createUploadId();
      const next: UploadItem = {
        ...item,
        id: uploadId,
        uploadId,
        uploadedBytes: 0,
        sizeBytes: item.kind === 'native' ? 0 : item.sizeBytes,
        copiedFiles: item.kind === 'native' ? 0 : item.copiedFiles,
        totalFiles: item.kind === 'native' ? 0 : item.totalFiles,
        status: 'queued',
        message: item.kind === 'native' ? 'Waiting to import…' : 'Waiting to upload…',
      };
      cancelledUploads.current.delete(item.id);
      cancelledUploads.current.delete(item.uploadId);
      setUploads((current) => current.map((upload) => (upload.id === item.id ? next : upload)));
      if (next.kind === 'native') void runNativeImport(next);
      else void runUpload(next);
    },
    [runNativeImport, runUpload],
  );

  // ----------------------------------------------------------- drag and drop
  /**
   * The OS drag/drop event arrives for the whole window, so every native drag
   * is hit-tested against the explorer here. Anywhere else — the editor, the
   * panel, the tab strip — is not a drop target and the drag is ignored.
   */
  const overExplorer = useCallback((position: DropPoint) => {
    const zone = explorer.current;
    if (!zone) return false;
    return isInsideDropZone(position, zone.getBoundingClientRect(), window.devicePixelRatio);
  }, []);

  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let active = true;

    void getCurrentWebviewWindow()
      .onDragDropEvent((event) => {
        switch (event.payload.type) {
          case 'enter':
          case 'over': {
            const inside = overExplorer(event.payload.position);
            dragDepth.current = inside ? 1 : 0;
            setDraggingOver(inside);
            break;
          }
          case 'drop': {
            const inside = overExplorer(event.payload.position);
            dragDepth.current = 0;
            setDraggingOver(false);
            // Dropped on the editor or the chrome: not a target, and importing
            // it would be a surprise the user cannot undo.
            if (inside) queueNativeImport(event.payload.paths);
            break;
          }
          case 'leave':
            dragDepth.current = 0;
            setDraggingOver(false);
            break;
        }
      })
      .then((next) => {
        if (active) {
          unlisten = next;
          nativeListenerReady.current = true;
        } else {
          next();
        }
      })
      // Registration failing is not fatal: it means this window is not a Tauri
      // webview, and the HTML5 handlers below take over instead.
      .catch((error: unknown) => report('Transfers', error));

    return () => {
      active = false;
      nativeListenerReady.current = false;
      unlisten?.();
    };
  }, [overExplorer, queueNativeImport, report]);

  // The four below are inert while the OS listener is live, which drives the
  // highlight itself and hit-tests the pointer properly. `preventDefault` still
  // runs unconditionally: without it the webview navigates away to the dropped
  // file, whichever path is doing the importing.
  const onDragEnter = useCallback((event: React.DragEvent<HTMLElement>) => {
    event.preventDefault();
    if (isInternalDrag(event) || !shouldImportBrowserDrop(nativeListenerReady.current)) return;
    dragDepth.current += 1;
    setDraggingOver(true);
  }, []);

  const onDragOver = useCallback((event: React.DragEvent<HTMLElement>) => {
    event.preventDefault();
    if (!isInternalDrag(event)) event.dataTransfer.dropEffect = 'copy';
  }, []);

  const onDragLeave = useCallback((event: React.DragEvent<HTMLElement>) => {
    event.preventDefault();
    if (isInternalDrag(event) || !shouldImportBrowserDrop(nativeListenerReady.current)) return;
    dragDepth.current = Math.max(0, dragDepth.current - 1);
    if (dragDepth.current === 0) setDraggingOver(false);
  }, []);

  const onDrop = useCallback(
    (event: React.DragEvent<HTMLElement>) => {
      event.preventDefault();
      // A drag that started inside the tree is a move, and the row it landed
      // on has already handled it.
      if (isInternalDrag(event)) return;
      // The OS listener has already imported this drop, hit-tested against the
      // explorer. Doing it again here would import everything twice.
      if (!shouldImportBrowserDrop(nativeListenerReady.current)) return;

      dragDepth.current = 0;
      setDraggingOver(false);
      collectDroppedItems(event.dataTransfer)
        .then(queueBrowserDrop)
        .catch((error: unknown) => report('Transfers', error));
    },
    [queueBrowserDrop, report],
  );

  // ------------------------------------------------------- running the project
  const runProject = useCallback(
    async (action: RunAction) => {
      setBusyAction(action);
      showPanel('terminal');
      const verbs: Record<RunAction, string> = {
        start: 'Starting',
        restart: 'Restarting',
        stop: 'Stopping',
        kill: 'Killing',
      };
      say('Project', `${verbs[action]} ${project.slug}…`);

      const call = {
        start: startProject,
        restart: restartProject,
        stop: stopProject,
        kill: killProject,
      }[action];

      try {
        await call(project.id);
        await onRefreshProjects();
        say('Project', `${project.slug}: ${action} finished.`);
      } catch (error) {
        report('Project', error);
      } finally {
        setBusyAction(null);
      }
    },
    [onRefreshProjects, project.id, project.slug, report, say, showPanel],
  );

  // --------------------------------------------------------------- searching
  const search = useMemo(() => searchProjectFiles, []);
  const sidebarSearch = useFileSearch(project.id, searchQuery, search, errorMessage);
  const quickOpen = useFileSearch(project.id, quickOpenTerm, search, errorMessage);

  const quickOpenPaths = useMemo(() => {
    if (quickOpenTerm.trim().length === 0) return editor.buffers.map((item) => item.path);
    return quickOpen.results.filter((entry) => entry.kind === 'file').map((entry) => entry.path);
  }, [editor.buffers, quickOpen.results, quickOpenTerm]);

  // --------------------------------------------------------------- commands
  const goTo = useCallback(
    (path: string, line: number, column: number) => {
      const jump = () => setReveal({ line, column, nonce: Date.now() });
      if (editor.active === path) {
        jump();
        return;
      }
      void openPath(path).then(jump);
    },
    [editor.active, openPath],
  );

  const commands = useMemo<Command[]>(() => {
    const hasFile = buffer !== null;
    const running = project.status === 'RUNNING';

    return [
      {
        id: 'file.new',
        title: 'New File…',
        category: 'File',
        keybinding: 'Ctrl+N',
        run: () => beginCreate(targetDirectory, false),
      },
      {
        id: 'file.newFolder',
        title: 'New Folder…',
        category: 'File',
        run: () => beginCreate(targetDirectory, true),
      },
      {
        id: 'file.save',
        title: 'Save',
        category: 'File',
        keybinding: 'Ctrl+S',
        enabled: hasFile && !buffer.readOnly,
        reason: hasFile ? 'This project is read-only right now' : 'No file is open',
        run: () => {
          if (buffer) void save(buffer.path);
        },
      },
      {
        id: 'file.saveAll',
        title: 'Save All',
        category: 'File',
        enabled: dirtyCount > 0,
        reason: 'Nothing has unsaved changes',
        run: () => {
          for (const item of editor.buffers.filter(isDirty)) void save(item.path);
        },
      },
      {
        id: 'file.revealExplorer',
        title: 'Reveal in File Explorer',
        category: 'File',
        enabled: hasFile,
        reason: 'No file is open',
        run: () => {
          if (buffer) void reveal_(buffer.path);
        },
      },
      {
        id: 'file.copyPath',
        title: 'Copy Path of Active File',
        category: 'File',
        enabled: hasFile,
        reason: 'No file is open',
        run: () => {
          if (buffer) copyToClipboard(absolutePath(projectRoot, buffer.path), 'the path');
        },
      },
      {
        id: 'file.close',
        title: 'Close Editor',
        category: 'File',
        keybinding: 'Ctrl+W',
        enabled: hasFile,
        reason: 'No file is open',
        run: () => {
          if (buffer) requestClose(buffer.path);
        },
      },
      {
        id: 'file.closeAll',
        title: 'Close All Editors',
        category: 'File',
        enabled: editor.buffers.length > 0,
        reason: 'No file is open',
        run: () => setEditor(closeAll()),
      },
      {
        id: 'view.toggleSidebar',
        title: 'Toggle Primary Side Bar',
        category: 'View',
        keybinding: 'Ctrl+B',
        run: () => patchLayout({ sidebarVisible: !layout.sidebarVisible }),
      },
      {
        id: 'view.togglePanel',
        title: 'Toggle Panel',
        category: 'View',
        keybinding: 'Ctrl+J',
        run: () => patchLayout({ panelVisible: !layout.panelVisible }),
      },
      {
        id: 'view.terminal',
        title: 'Toggle Terminal',
        category: 'View',
        keybinding: 'Ctrl+`',
        run: () =>
          patchLayout({
            panelVisible: !(layout.panelVisible && layout.panelTab === 'terminal'),
            panelTab: 'terminal',
          }),
      },
      {
        id: 'view.problems',
        title: 'Show Problems',
        category: 'View',
        keybinding: 'Ctrl+Shift+M',
        run: () => showPanel('problems'),
      },
      {
        id: 'view.output',
        title: 'Show Output',
        category: 'View',
        run: () => showPanel('output'),
      },
      {
        id: 'view.explorer',
        title: 'Show Explorer',
        category: 'View',
        keybinding: 'Ctrl+Shift+E',
        run: () => showSidebar('explorer'),
      },
      {
        id: 'view.search',
        title: 'Search Files by Name',
        category: 'View',
        keybinding: 'Ctrl+Shift+F',
        run: () => showSidebar('search'),
      },
      {
        id: 'explorer.refresh',
        title: 'Refresh Explorer',
        category: 'Explorer',
        run: () => {
          for (const directory of ['', ...expanded]) void loadDirectory(directory);
          say('Files', 'Refreshed the explorer.');
        },
      },
      {
        id: 'explorer.collapse',
        title: 'Collapse Folders in Explorer',
        category: 'Explorer',
        run: () => setExpanded([]),
      },
      {
        id: 'project.start',
        title: 'Start Project',
        category: 'Run',
        enabled: !running && dockerAvailable && busyAction === null,
        reason: dockerAvailable ? 'The project is already running' : 'Docker is not available',
        run: () => void runProject('start'),
      },
      {
        id: 'project.stop',
        title: 'Stop Project',
        category: 'Run',
        enabled: running && busyAction === null,
        reason: 'The project is not running',
        run: () => void runProject('stop'),
      },
      {
        id: 'project.restart',
        title: 'Restart Project',
        category: 'Run',
        enabled: running && busyAction === null,
        reason: 'The project is not running',
        run: () => void runProject('restart'),
      },
      {
        id: 'project.kill',
        title: 'Kill Project',
        category: 'Run',
        enabled: running && busyAction === null,
        reason: 'The project is not running',
        run: () => void runProject('kill'),
      },
      {
        id: 'project.close',
        title: 'Close Project',
        category: 'Go',
        run: leave,
      },
      {
        id: 'app.settings',
        title: 'Open Settings',
        category: 'Preferences',
        run: onOpenSettings,
      },
    ];
  }, [
    beginCreate,
    buffer,
    busyAction,
    copyToClipboard,
    dirtyCount,
    dockerAvailable,
    editor.buffers,
    expanded,
    layout.panelTab,
    layout.panelVisible,
    layout.sidebarVisible,
    loadDirectory,
    onOpenSettings,
    patchLayout,
    project.status,
    projectRoot,
    requestClose,
    reveal_,
    runProject,
    save,
    say,
    showPanel,
    showSidebar,
    targetDirectory,
  ]);

  function leave() {
    if (dirtyCount > 0) {
      setConfirm({
        title: `${dirtyCount} file${dirtyCount === 1 ? '' : 's'} ${dirtyCount === 1 ? 'has' : 'have'} unsaved changes.`,
        detail: 'Closing the project will discard them.',
        confirmLabel: 'Close anyway',
        danger: true,
        onConfirm: () => {
          setConfirm(null);
          onLeave();
        },
      });
      return;
    }
    onLeave();
  }

  // Held in a ref so the shortcut handler and the toolbar buttons can run a
  // command by id without every one of them re-binding on each render.
  const commandsRef = useRef(commands);
  commandsRef.current = commands;

  const runCommand = useCallback((id: string) => {
    // Looked up at call time so the command's own `enabled` is the current one.
    const command = commandsRef.current.find((entry) => entry.id === id);
    if (command && command.enabled !== false) command.run();
  }, []);

  // ------------------------------------------------------------- shortcuts
  useEffect(() => {
    function onKeyDown(event: KeyboardEvent) {
      const modifier = event.ctrlKey || event.metaKey;
      const typing = isTypingTarget(event.target);

      if (modifier && !event.shiftKey && event.key.toLowerCase() === 's') {
        event.preventDefault();
        if (editor.active !== null) void save(editor.active);
        return;
      }
      if (modifier && event.shiftKey && event.key.toLowerCase() === 'p') {
        event.preventDefault();
        setPalette('commands');
        return;
      }
      if (modifier && !event.shiftKey && event.key.toLowerCase() === 'p') {
        event.preventDefault();
        setPalette('files');
        return;
      }
      if (modifier && !event.shiftKey && event.key.toLowerCase() === 'b') {
        event.preventDefault();
        runCommand('view.toggleSidebar');
        return;
      }
      if (modifier && !event.shiftKey && event.key.toLowerCase() === 'j') {
        event.preventDefault();
        runCommand('view.togglePanel');
        return;
      }
      if (modifier && event.key === '`') {
        event.preventDefault();
        runCommand('view.terminal');
        return;
      }
      if (modifier && !event.shiftKey && event.key.toLowerCase() === 'w') {
        event.preventDefault();
        if (editor.active !== null) requestClose(editor.active);
        return;
      }
      if (modifier && !event.shiftKey && event.key.toLowerCase() === 'n') {
        event.preventDefault();
        runCommand('file.new');
        return;
      }
      if (modifier && event.shiftKey && event.key.toLowerCase() === 'f') {
        event.preventDefault();
        runCommand('view.search');
        return;
      }
      if (modifier && event.shiftKey && event.key.toLowerCase() === 'e') {
        event.preventDefault();
        runCommand('view.explorer');
        return;
      }
      if (modifier && event.shiftKey && event.key.toLowerCase() === 'm') {
        event.preventDefault();
        runCommand('view.problems');
        return;
      }

      // The two that act on the tree act only when the tree is what has focus:
      // Delete inside the editor deletes a character, and must go on doing so.
      if (typing) return;
      const entry = selected === null ? null : findEntry(listings, selected);
      if (event.key === 'F2' && entry) {
        event.preventDefault();
        beginRename(entry);
        return;
      }
      if (event.key === 'Delete' && entry) {
        event.preventDefault();
        remove(entry);
      }
    }

    window.addEventListener('keydown', onKeyDown);
    return () => window.removeEventListener('keydown', onKeyDown);
  }, [beginRename, editor.active, listings, remove, requestClose, runCommand, save, selected]);

  // ------------------------------------------------------------------ menus
  function commandEntry(id: string): MenuEntry {
    const command = commands.find((entry) => entry.id === id);
    if (!command) return { id, separator: true };
    return {
      id: command.id,
      label: command.title,
      keybinding: command.keybinding,
      enabled: command.enabled,
      run: command.run,
    };
  }

  const menus: Menu[] = [
    {
      id: 'file',
      label: 'File',
      entries: [
        commandEntry('file.new'),
        commandEntry('file.newFolder'),
        { id: 'file.sep1', separator: true },
        commandEntry('file.save'),
        commandEntry('file.saveAll'),
        { id: 'file.sep2', separator: true },
        commandEntry('file.revealExplorer'),
        commandEntry('file.copyPath'),
        { id: 'file.sep3', separator: true },
        commandEntry('file.close'),
        commandEntry('file.closeAll'),
        commandEntry('project.close'),
      ],
    },
    {
      id: 'edit',
      label: 'Edit',
      entries: [
        editorAction('edit.undo', 'Undo', 'Ctrl+Z', 'undo'),
        editorAction('edit.redo', 'Redo', 'Ctrl+Y', 'redo'),
        { id: 'edit.sep1', separator: true },
        editorAction('edit.cut', 'Cut', 'Ctrl+X', 'editor.action.clipboardCutAction'),
        editorAction('edit.copy', 'Copy', 'Ctrl+C', 'editor.action.clipboardCopyAction'),
        editorAction('edit.paste', 'Paste', 'Ctrl+V', 'editor.action.clipboardPasteAction'),
        { id: 'edit.sep2', separator: true },
        editorAction('edit.find', 'Find', 'Ctrl+F', 'actions.find'),
        editorAction('edit.replace', 'Replace', 'Ctrl+H', 'editor.action.startFindReplaceAction'),
      ],
    },
    {
      id: 'selection',
      label: 'Selection',
      entries: [
        editorAction('selection.all', 'Select All', 'Ctrl+A', 'editor.action.selectAll'),
        editorAction(
          'selection.expand',
          'Expand Selection',
          'Shift+Alt+Right',
          'editor.action.smartSelect.expand',
        ),
        { id: 'selection.sep', separator: true },
        editorAction(
          'selection.copyUp',
          'Copy Line Up',
          'Shift+Alt+Up',
          'editor.action.copyLinesUpAction',
        ),
        editorAction(
          'selection.moveDown',
          'Move Line Down',
          'Alt+Down',
          'editor.action.moveLinesDownAction',
        ),
      ],
    },
    {
      id: 'view',
      label: 'View',
      entries: [
        commandEntry('view.explorer'),
        commandEntry('view.search'),
        { id: 'view.sep1', separator: true },
        commandEntry('view.problems'),
        commandEntry('view.output'),
        commandEntry('view.terminal'),
        { id: 'view.sep2', separator: true },
        commandEntry('view.toggleSidebar'),
        commandEntry('view.togglePanel'),
      ],
    },
    {
      id: 'go',
      label: 'Go',
      entries: [
        {
          id: 'go.file',
          label: 'Go to File…',
          keybinding: 'Ctrl+P',
          run: () => setPalette('files'),
        },
        {
          id: 'go.command',
          label: 'Show All Commands',
          keybinding: 'Ctrl+Shift+P',
          run: () => setPalette('commands'),
        },
        { id: 'go.sep', separator: true },
        editorAction('go.line', 'Go to Line…', 'Ctrl+G', 'editor.action.gotoLine'),
        { id: 'go.sep2', separator: true },
        {
          id: 'go.back',
          label: 'Back',
          enabled: history.index > 0,
          run: () => navigate(-1),
        },
        {
          id: 'go.forward',
          label: 'Forward',
          enabled: history.index < history.paths.length - 1,
          run: () => navigate(1),
        },
      ],
    },
    {
      id: 'run',
      label: 'Run',
      entries: [
        commandEntry('project.start'),
        commandEntry('project.restart'),
        commandEntry('project.stop'),
        commandEntry('project.kill'),
      ],
    },
    {
      id: 'terminal',
      label: 'Terminal',
      entries: [commandEntry('view.terminal'), commandEntry('view.output')],
    },
    {
      id: 'help',
      label: 'Help',
      entries: [
        {
          id: 'help.version',
          label: `Version ${status?.appVersion ?? '—'}`,
          enabled: false,
          run: () => {},
        },
        commandEntry('app.settings'),
        {
          id: 'help.update',
          label:
            update.check?.state === 'available'
              ? `Install update ${update.check.newVersion}`
              : 'Check for updates',
          run: () => {
            if (update.check?.state === 'available') void updateStore.install();
            else void updateStore.check();
          },
        },
      ],
    },
  ];

  /**
   * An action Monaco already provides, reached through Monaco.
   *
   * Undo, find and the rest are the editor's own commands. Triggering them
   * rather than reimplementing them is what makes the menu entry behave
   * identically to the shortcut — including the undo stack being per file.
   */
  function editorAction(id: string, label: string, keybinding: string, action: string): MenuEntry {
    return {
      id,
      label,
      keybinding,
      enabled: buffer !== null,
      run: () => {
        const [instance] = monaco.editor.getEditors();
        if (!instance) return;
        instance.focus();
        instance.trigger('menu', action, null);
      },
    };
  }

  function navigate(direction: -1 | 1) {
    setHistory((current) => {
      const index = current.index + direction;
      const path = current.paths[index];
      if (path === undefined) return current;
      if (bufferFor(editor, path)) {
        setEditor((state) => ({ ...state, active: path }));
        setSelected(path);
      } else {
        void openPath(path);
      }
      return { ...current, index };
    });
  }

  // ----------------------------------------------------------- context menus
  function fileMenu(entry: FileEntry, event: React.MouseEvent) {
    event.preventDefault();
    event.stopPropagation();
    setSelected(entry.path);
    if (entry.kind === 'directory') setTargetDirectory(entry.path);

    const directory = entry.kind === 'directory' ? entry.path : parentOf(entry.path);
    setMenu({
      position: { x: event.clientX, y: event.clientY },
      entries: [
        {
          id: 'new-file',
          label: 'New File…',
          icon: 'new-file',
          run: () => beginCreate(directory, false),
        },
        {
          id: 'new-folder',
          label: 'New Folder…',
          icon: 'new-folder',
          run: () => beginCreate(directory, true),
        },
        { id: 'sep1', separator: true },
        {
          id: 'reveal',
          label: 'Reveal in File Explorer',
          icon: 'external',
          run: () => void reveal_(entry.path),
        },
        {
          id: 'copy-path',
          label: 'Copy Path',
          icon: 'copy',
          run: () => copyToClipboard(absolutePath(projectRoot, entry.path), 'the path'),
        },
        {
          id: 'copy-relative',
          label: 'Copy Relative Path',
          run: () => copyToClipboard(entry.path, 'the relative path'),
        },
        { id: 'sep2', separator: true },
        {
          id: 'rename',
          label: 'Rename…',
          icon: 'pencil',
          keybinding: 'F2',
          run: () => beginRename(entry),
        },
        {
          id: 'duplicate',
          label: 'Duplicate',
          icon: 'copy',
          run: () => void duplicate(entry),
        },
        {
          id: 'delete',
          label: 'Delete',
          icon: 'trash',
          keybinding: 'Delete',
          danger: true,
          run: () => remove(entry),
        },
      ],
    });
  }

  function rootMenu(event: React.MouseEvent) {
    event.preventDefault();
    setSelected(null);
    setTargetDirectory('');
    setMenu({
      position: { x: event.clientX, y: event.clientY },
      entries: [
        {
          id: 'new-file',
          label: 'New File…',
          icon: 'new-file',
          run: () => beginCreate('', false),
        },
        {
          id: 'new-folder',
          label: 'New Folder…',
          icon: 'new-folder',
          run: () => beginCreate('', true),
        },
        { id: 'sep', separator: true },
        {
          id: 'refresh',
          label: 'Refresh Explorer',
          icon: 'refresh',
          run: () => runCommand('explorer.refresh'),
        },
        {
          id: 'reveal',
          label: 'Reveal in File Explorer',
          icon: 'external',
          run: () => void reveal_(''),
        },
      ],
    });
  }

  function tabMenu(path: string, event: React.MouseEvent) {
    event.preventDefault();
    const index = editor.buffers.findIndex((item) => item.path === path);
    setMenu({
      position: { x: event.clientX, y: event.clientY },
      entries: [
        { id: 'close', label: 'Close', keybinding: 'Ctrl+W', run: () => requestClose(path) },
        {
          id: 'close-others',
          label: 'Close Others',
          enabled: editor.buffers.length > 1,
          run: () => setEditor((current) => closeOthers(current, path)),
        },
        {
          id: 'close-right',
          label: 'Close to the Right',
          enabled: index >= 0 && index < editor.buffers.length - 1,
          run: () => setEditor((current) => closeToRight(current, path)),
        },
        { id: 'close-all', label: 'Close All', run: () => setEditor(closeAll()) },
        { id: 'sep', separator: true },
        {
          id: 'copy-path',
          label: 'Copy Path',
          icon: 'copy',
          run: () => copyToClipboard(absolutePath(projectRoot, path), 'the path'),
        },
        {
          id: 'reveal',
          label: 'Reveal in File Explorer',
          icon: 'external',
          run: () => void reveal_(path),
        },
        {
          id: 'reveal-explorer',
          label: 'Reveal in Explorer View',
          run: () => {
            showSidebar('explorer');
            setSelected(path);
            const directory = parentOf(path);
            if (directory) {
              setExpanded((current) =>
                current.includes(directory) ? current : [...current, directory],
              );
              void loadDirectory(directory);
            }
          },
        },
      ],
    });
  }

  // ------------------------------------------------------------------ render
  const treeState: TreeState = {
    listings,
    expanded,
    selected,
    targetDirectory,
    editing,
  };

  const welcomeActions: WelcomeAction[] = [
    {
      id: 'new',
      label: 'New File…',
      icon: 'new-file',
      keybinding: 'Ctrl+N',
      run: () => runCommand('file.new'),
    },
    {
      id: 'open',
      label: 'Go to File…',
      icon: 'search',
      keybinding: 'Ctrl+P',
      run: () => setPalette('files'),
    },
    {
      id: 'explorer',
      label: 'Show Explorer',
      icon: 'file',
      keybinding: 'Ctrl+Shift+E',
      run: () => runCommand('view.explorer'),
    },
    {
      id: 'terminal',
      label: 'Open Terminal Panel',
      icon: 'terminal',
      keybinding: 'Ctrl+`',
      run: () => showPanel('terminal'),
    },
    {
      id: 'run',
      label: 'Start Project',
      icon: 'play',
      enabled: project.status !== 'RUNNING' && dockerAvailable && busyAction === null,
      run: () => runCommand('project.start'),
    },
  ];

  return (
    <div className="vs-root flex h-full min-h-0 flex-col bg-vs-editor">
      <TitleBar
        menus={menus}
        projectName={project.displayName}
        canGoBack={history.index > 0}
        canGoForward={history.index < history.paths.length - 1}
        onBack={() => navigate(-1)}
        onForward={() => navigate(1)}
        onOpenPalette={() => setPalette('files')}
        update={
          update.check?.state === 'available'
            ? {
                label: buttonLabel(update.phase),
                busy: !canStart(update.phase),
                onInstall: () => void updateStore.install(),
              }
            : null
        }
        sidebarVisible={layout.sidebarVisible}
        panelVisible={layout.panelVisible}
        onToggleSidebar={() => runCommand('view.toggleSidebar')}
        onTogglePanel={() => runCommand('view.togglePanel')}
      />

      <div className="flex min-h-0 flex-1">
        <ActivityBar
          view={layout.activityView}
          visible={layout.sidebarVisible}
          unsaved={dirtyCount}
          onSelect={(view) =>
            patchLayout(
              layout.activityView === view && layout.sidebarVisible
                ? { sidebarVisible: false }
                : { sidebarVisible: true, activityView: view },
            )
          }
          onProjects={leave}
          onSettings={onOpenSettings}
        />

        {layout.sidebarVisible && (
          <>
            <aside
              style={{ width: `${layout.sidebarWidth}px` }}
              className="flex min-h-0 shrink-0 flex-col bg-vs-sidebar"
            >
              {layout.activityView === 'explorer' && (
                <ExplorerPanel
                  ref={explorer}
                  projectName={project.displayName}
                  state={treeState}
                  callbacks={{
                    onOpen: (entry) => void openPath(entry.path),
                    onToggle: (path) => {
                      setExpanded((current) => toggleExpanded(current, path));
                      if (!listings[path]) void loadDirectory(path);
                    },
                    onContextMenu: fileMenu,
                    onMove: (from, to) => void move(from, to),
                    onSelectDirectory: (path) => {
                      setTargetDirectory(path);
                      setSelected(path);
                    },
                    onSelectFile: setSelected,
                  }}
                  actions={{
                    onNewFile: () => beginCreate(targetDirectory, false),
                    onNewFolder: () => beginCreate(targetDirectory, true),
                    onRefresh: () => runCommand('explorer.refresh'),
                    onCollapseAll: () => setExpanded([]),
                  }}
                  uploads={uploads}
                  draggingOver={draggingOver}
                  dropTargetLabel={displayDirectory(targetDirectory)}
                  onSubmitEdit={(value) => void submitEdit(value)}
                  onCancelEdit={() => setEditing(null)}
                  onCancelUpload={cancelUpload}
                  onRetryUpload={retryUpload}
                  onClearFinishedUploads={() =>
                    setUploads((current) =>
                      current.filter(
                        (item) => item.status === 'queued' || item.status === 'uploading',
                      ),
                    )
                  }
                  onEmptyAreaClick={() => {
                    setSelected(null);
                    setTargetDirectory('');
                  }}
                  onEmptyAreaContextMenu={rootMenu}
                  onDragEnter={onDragEnter}
                  onDragLeave={onDragLeave}
                  onDragOver={onDragOver}
                  onDrop={onDrop}
                />
              )}

              {layout.activityView === 'search' && (
                <SearchPanel
                  query={searchQuery}
                  onQueryChange={setSearchQuery}
                  results={sidebarSearch.results}
                  searching={sidebarSearch.searching}
                  failure={sidebarSearch.failure}
                  onOpen={(entry) => {
                    if (entry.kind === 'directory') {
                      setTargetDirectory(entry.path);
                      showSidebar('explorer');
                      setExpanded((current) =>
                        current.includes(entry.path) ? current : [...current, entry.path],
                      );
                      void loadDirectory(entry.path);
                      return;
                    }
                    void openPath(entry.path);
                  }}
                />
              )}

              {layout.activityView === 'source-control' && (
                <SourceControlPanel projectRoot={projectRoot} />
              )}
              {layout.activityView === 'run' && (
                <RunPanel
                  project={project}
                  dockerAvailable={dockerAvailable}
                  onOpenTerminal={() => showPanel('terminal')}
                />
              )}
              {layout.activityView === 'extensions' && <ExtensionsPanel />}
              {layout.activityView === 'account' && (
                <AccountPanel status={status} openFiles={editor.buffers.map((item) => item.path)} />
              )}
            </aside>

            <ResizeHandle
              orientation="vertical"
              label="Resize the side bar"
              value={layout.sidebarWidth}
              onResize={(next, source) =>
                patchLayout({
                  sidebarWidth: clampSidebarWidth(
                    // A pointer reports where it is; the keyboard reports a
                    // size. The activity bar's 48px is the difference.
                    source === 'pointer' ? next - 48 : next,
                    window.innerWidth,
                  ),
                })
              }
              onDoubleClick={() => patchLayout({ sidebarVisible: false })}
            />
          </>
        )}

        <main className="flex min-w-0 flex-1 flex-col">
          <div className="flex min-h-0 flex-1 flex-col">
            {editor.buffers.length > 0 && (
              <EditorTabs
                buffers={editor.buffers}
                active={editor.active}
                onSelect={(path) => {
                  setEditor((current) => ({ ...current, active: path }));
                  setSelected(path);
                }}
                onClose={requestClose}
                onContextMenu={tabMenu}
              />
            )}

            {buffer?.readOnly && (
              <p className="flex shrink-0 items-center gap-1.5 border-b border-amber-900/60 bg-amber-950/40 px-3 py-1 text-[12px] text-amber-300">
                <Icon name="warning" size={13} />
                This project is being built or removed, so its files are read-only right now.
              </p>
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
                  onCursor={setCursor}
                  reveal={reveal}
                />
              ) : (
                <EmptyEditorState projectName={project.displayName} actions={welcomeActions} />
              )}
            </div>
          </div>

          {layout.panelVisible && (
            <>
              <ResizeHandle
                orientation="horizontal"
                label="Resize the panel"
                value={layout.panelHeight}
                onResize={(next, source) =>
                  patchLayout({
                    panelHeight: clampPanelHeight(
                      source === 'pointer' ? window.innerHeight - next - 22 : next,
                      window.innerHeight,
                    ),
                  })
                }
                onDoubleClick={() => patchLayout({ panelVisible: false })}
              />
              <div style={{ height: `${layout.panelHeight}px` }} className="flex min-h-0 shrink-0">
                <BottomPanel
                  tab={layout.panelTab}
                  onSelect={(tab) => patchLayout({ panelTab: tab })}
                  onClose={() => patchLayout({ panelVisible: false })}
                  problemCount={errors + warnings}
                  children={{
                    problems: (
                      <ProblemsPanel
                        problems={problems}
                        hasOpenFiles={editor.buffers.length > 0}
                        onSelect={(problem: Problem) =>
                          goTo(problem.path, problem.line, problem.column)
                        }
                      />
                    ),
                    output: (
                      <OutputPanel
                        lines={linesForChannel(output, outputChannel)}
                        channel={outputChannel}
                        onSelectChannel={setOutputChannel}
                        onClear={() => setOutput([])}
                      />
                    ),
                    terminal: (
                      <TerminalPanel
                        project={project}
                        dockerAvailable={dockerAvailable}
                        busy={busyAction}
                        onAction={(action) => void runProject(action)}
                        lines={linesForChannel(output, 'Project')}
                      />
                    ),
                    logs: (
                      <LogsPanel
                        settings={settings}
                        onCopyPath={(path) => copyToClipboard(path, 'the path')}
                      />
                    ),
                  }}
                />
              </div>
            </>
          )}
        </main>
      </div>

      <StatusBar
        projectStatus={project.status}
        running={project.status === 'RUNNING'}
        dockerAvailable={dockerAvailable}
        errors={errors}
        warnings={warnings}
        language={buffer ? languageName(buffer.language) : null}
        cursor={buffer ? cursor : null}
        lineEnding={buffer ? (buffer.current.includes('\r\n') ? 'CRLF' : 'LF') : null}
        dirty={buffer !== null && isDirty(buffer)}
        saving={saving}
        onProblems={() => showPanel('problems')}
        onRun={() => showPanel('terminal')}
      />

      {palette && (
        <CommandPalette
          mode={palette}
          commands={commands}
          paths={quickOpenPaths}
          loadingPaths={quickOpen.searching}
          onFileQuery={setQuickOpenTerm}
          onRunCommand={(command) => command.run()}
          onOpenPath={(path) => void openPath(path)}
          onClose={() => setPalette(null)}
        />
      )}

      {menu && (
        <ContextMenu
          entries={menu.entries}
          position={menu.position}
          onClose={() => setMenu(null)}
        />
      )}

      {conflict && (
        <ConflictDialog
          conflicts={conflict.paths}
          targetDirectory={conflict.directory}
          onChoose={(choice) => conflict.resolve(choice)}
          onCancel={() => conflict.resolve(null)}
        />
      )}

      {confirm && (
        <ConfirmDialog
          title={confirm.title}
          detail={confirm.detail}
          confirmLabel={confirm.confirmLabel}
          danger={confirm.danger}
          onConfirm={confirm.onConfirm}
          onCancel={() => setConfirm(null)}
        />
      )}
    </div>
  );
}

/** The absolute path of a project file, for display and for the clipboard. */
function absolutePath(root: string | null, path: string): string {
  if (!root) return path;
  const separator = root.includes('\\') ? '\\' : '/';
  const tail = separator === '\\' ? path.replace(/\//g, '\\') : path;
  return path ? `${root}${separator}${tail}` : root;
}

/** The entry for a path, from whichever listing holds it. */
function findEntry(listings: Record<string, FileEntry[]>, path: string): FileEntry | null {
  for (const entries of Object.values(listings)) {
    const found = entries.find((entry) => entry.path === path);
    if (found) return found;
  }
  return null;
}

function isInternalDrag(event: React.DragEvent): boolean {
  return event.dataTransfer.types.includes('application/x-project-path');
}

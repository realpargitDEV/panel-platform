/**
 * The tree itself.
 *
 * One row per entry, indented by depth, recursing into expanded folders. Rows
 * are `div`s with tree roles rather than nested buttons: a row has to be a drop
 * target, a right-click target, a drag source and a selection target at once,
 * and a button that contains other buttons is neither valid nor navigable.
 *
 * Selection lives in one model above this component — see `selection.ts`. Rows
 * ask whether they are selected; they do not each remember it. Every row also
 * carries `data-row` and its path, which is what the rubber band uses to find
 * the boxes it has swept over.
 *
 * Rendering is deliberately not virtualised. The core caps a listing before it
 * returns it, so no single folder can be long enough to need it.
 */
import { useState } from 'react';

import type { FileEntry } from '../api';
import Icon from './Icon';
import { fileIconColor } from './fileIcons';
import { isSelected, type Selection } from './selection';
import { parentOf } from './tabs';

export interface TreeCallbacks {
  onOpen: (entry: FileEntry) => void;
  onToggle: (path: string) => void;
  onContextMenu: (entry: FileEntry, event: React.MouseEvent) => void;
  /** A pointer press on a row, carrying the modifier keys. */
  onRowPointerDown: (entry: FileEntry, event: React.MouseEvent) => void;
  /** Files and folders dragged from inside the tree onto a folder row. */
  onMove: (paths: string[], toDirectory: string) => void;
  /** What a drag starting on this row should carry. */
  dragPathsFor: (path: string) => string[];
  /** The folder new files and drops go into. */
  onSelectDirectory: (path: string) => void;
}

export interface TreeState {
  listings: Record<string, FileEntry[]>;
  expanded: string[];
  selection: Selection;
  /** The folder new files and drops go into. */
  targetDirectory: string;
  /** The path being renamed inline, or the folder a new entry is being named in. */
  editing: InlineEdit | null;
}

export interface InlineEdit {
  kind: 'rename' | 'create-file' | 'create-folder';
  /** For a rename: the entry. For a creation: the parent folder. */
  path: string;
  initialValue: string;
}

/** How far one level indents. VS Code's own value. */
const INDENT = 8;
const ROW_PADDING = 12;

/** The type a drag from inside the tree carries. */
export const INTERNAL_DRAG_TYPE = 'application/x-project-path';

export default function FileTree({
  directory,
  depth,
  state,
  callbacks,
  onSubmitEdit,
  onCancelEdit,
}: {
  directory: string;
  depth: number;
  state: TreeState;
  callbacks: TreeCallbacks;
  onSubmitEdit: (value: string) => void;
  onCancelEdit: () => void;
}) {
  const entries = state.listings[directory];
  const creating =
    state.editing !== null && state.editing.kind !== 'rename' && state.editing.path === directory;

  if (!entries && !creating) return null;

  return (
    <div role={depth === 0 ? 'tree' : 'group'} aria-label={depth === 0 ? 'Files' : undefined}>
      {creating && state.editing && (
        <InlineInput
          depth={depth}
          icon={state.editing.kind === 'create-folder' ? 'folder' : 'file'}
          initialValue=""
          onSubmit={onSubmitEdit}
          onCancel={onCancelEdit}
        />
      )}

      {(entries ?? []).map((entry) => (
        <Row
          key={entry.path}
          entry={entry}
          depth={depth}
          state={state}
          callbacks={callbacks}
          onSubmitEdit={onSubmitEdit}
          onCancelEdit={onCancelEdit}
        />
      ))}
    </div>
  );
}

function Row({
  entry,
  depth,
  state,
  callbacks,
  onSubmitEdit,
  onCancelEdit,
}: {
  entry: FileEntry;
  depth: number;
  state: TreeState;
  callbacks: TreeCallbacks;
  onSubmitEdit: (value: string) => void;
  onCancelEdit: () => void;
}) {
  const [dropping, setDropping] = useState(false);
  const isDirectory = entry.kind === 'directory';
  const isOpen = state.expanded.includes(entry.path);
  const selected = isSelected(state.selection, entry.path);
  const focused = state.selection.focused === entry.path;
  const isTarget = isDirectory && state.targetDirectory === entry.path;
  const renaming =
    state.editing?.kind === 'rename' && state.editing.path === entry.path ? state.editing : null;

  if (renaming) {
    return (
      <InlineInput
        depth={depth}
        icon={isDirectory ? 'folder' : 'file'}
        initialValue={renaming.initialValue}
        onSubmit={onSubmitEdit}
        onCancel={onCancelEdit}
      />
    );
  }

  /** Where an item dropped on this row belongs: into a folder, beside a file. */
  const dropDirectory = isDirectory ? entry.path : parentOf(entry.path);

  function onDragStart(event: React.DragEvent) {
    // A private type: a drag from the tree must be told apart from a file
    // dragged in from the desktop, which is an import rather than a move.
    const paths = callbacks.dragPathsFor(entry.path);
    event.dataTransfer.setData(INTERNAL_DRAG_TYPE, JSON.stringify(paths));
    event.dataTransfer.effectAllowed = 'move';

    // A drag of several rows needs to say so; the browser would otherwise show
    // the one row the pointer happened to be on.
    if (paths.length > 1) {
      const badge = document.createElement('div');
      badge.textContent = `${paths.length} items`;
      badge.style.cssText =
        'position:absolute;top:-1000px;left:-1000px;padding:4px 8px;border-radius:4px;' +
        'background:#3b82f6;color:#fff;font:500 12px system-ui;';
      document.body.append(badge);
      event.dataTransfer.setDragImage(badge, 12, 12);
      // Removed on the next frame: the browser has taken its snapshot by then,
      // and leaving it would litter the document with one node per drag.
      requestAnimationFrame(() => badge.remove());
    }
  }

  function isInternalDrag(event: React.DragEvent): boolean {
    return event.dataTransfer.types.includes(INTERNAL_DRAG_TYPE);
  }

  function onDragOver(event: React.DragEvent) {
    if (!isInternalDrag(event)) return;
    event.preventDefault();
    event.stopPropagation();
    event.dataTransfer.dropEffect = 'move';
    setDropping(true);
  }

  function onDrop(event: React.DragEvent) {
    if (!isInternalDrag(event)) return;
    event.preventDefault();
    event.stopPropagation();
    setDropping(false);

    const raw = event.dataTransfer.getData(INTERNAL_DRAG_TYPE);
    if (!raw) return;
    try {
      const paths: unknown = JSON.parse(raw);
      if (Array.isArray(paths)) {
        callbacks.onMove(
          paths.filter((path): path is string => typeof path === 'string'),
          dropDirectory,
        );
      }
    } catch {
      // A drag carrying something this build does not understand. Ignoring it
      // beats moving files based on a guess.
    }
  }

  return (
    <div>
      <div
        data-row
        data-path={entry.path}
        role="treeitem"
        tabIndex={-1}
        aria-expanded={isDirectory ? isOpen : undefined}
        aria-selected={selected}
        aria-level={depth + 1}
        title={entry.kind === 'other' ? `${entry.path} — a link or special file` : entry.path}
        draggable
        onDragStart={onDragStart}
        onDragOver={onDragOver}
        onDragLeave={() => setDropping(false)}
        onDrop={onDrop}
        onContextMenu={(event) => callbacks.onContextMenu(entry, event)}
        onMouseDown={(event) => callbacks.onRowPointerDown(entry, event)}
        onClick={(event) => {
          // A modified click only changed the selection, which the pointer-down
          // handler has already done.
          if (event.ctrlKey || event.metaKey || event.shiftKey) return;
          if (isDirectory) {
            callbacks.onSelectDirectory(entry.path);
            callbacks.onToggle(entry.path);
            return;
          }
          if (entry.kind === 'file') callbacks.onOpen(entry);
        }}
        style={{ paddingLeft: `${ROW_PADDING + depth * INDENT}px` }}
        className={`group flex h-[22px] cursor-pointer items-center gap-1 pr-2 select-none ${
          selected
            ? 'bg-vs-active text-white'
            : isTarget
              ? 'bg-vs-selected text-vs-text'
              : 'text-vs-text hover:bg-white/5'
        } ${focused ? 'outline outline-1 -outline-offset-1 outline-accent' : ''} ${
          dropping ? 'vs-drop-target' : ''
        }`}
      >
        <span className="w-3.5 shrink-0 text-vs-dim">
          {isDirectory && <Icon name={isOpen ? 'chevron-down' : 'chevron-right'} size={14} />}
        </span>

        <span
          className="shrink-0"
          style={{ color: isDirectory ? '#8aa2c8' : fileIconColor(entry.name) }}
        >
          <Icon
            name={
              entry.kind === 'other'
                ? 'blocked'
                : isDirectory
                  ? isOpen
                    ? 'folder-open'
                    : 'folder'
                  : 'file'
            }
            size={15}
          />
        </span>

        <span
          className={`truncate ${entry.kind === 'other' ? 'text-vs-dim italic' : ''} ${
            entry.isSymlink ? 'italic' : ''
          }`}
        >
          {entry.name}
        </span>
      </div>

      {isDirectory && isOpen && (
        <FileTree
          directory={entry.path}
          depth={depth + 1}
          state={state}
          callbacks={callbacks}
          onSubmitEdit={onSubmitEdit}
          onCancelEdit={onCancelEdit}
        />
      )}
    </div>
  );
}

/**
 * The row that becomes a text box: a rename in place, or a new file being
 * named where it will appear.
 *
 * Blur commits nothing — it cancels. Clicking away from a half-typed name is
 * far more often "never mind" than "yes, create that", and creating on blur
 * makes a stray click leave files behind.
 */
function InlineInput({
  depth,
  icon,
  initialValue,
  onSubmit,
  onCancel,
}: {
  depth: number;
  icon: 'file' | 'folder';
  initialValue: string;
  onSubmit: (value: string) => void;
  onCancel: () => void;
}) {
  const [value, setValue] = useState(initialValue);

  return (
    <form
      onSubmit={(event) => {
        event.preventDefault();
        const trimmed = value.trim();
        if (trimmed.length === 0) {
          onCancel();
          return;
        }
        onSubmit(trimmed);
      }}
      style={{ paddingLeft: `${ROW_PADDING + depth * INDENT}px` }}
      className="flex h-[22px] items-center gap-1 pr-2"
    >
      <span className="w-3.5 shrink-0" />
      <span className="shrink-0 text-vs-dim">
        <Icon name={icon} size={15} />
      </span>
      <input
        autoFocus
        value={value}
        onChange={(event) => setValue(event.target.value)}
        onKeyDown={(event) => {
          if (event.key === 'Escape') {
            event.preventDefault();
            event.stopPropagation();
            onCancel();
          }
        }}
        onBlur={onCancel}
        onFocus={(event) => {
          // Select the stem so typing replaces the name but keeps `.ts`.
          const dot = event.target.value.lastIndexOf('.');
          event.target.setSelectionRange(0, dot > 0 ? dot : event.target.value.length);
        }}
        aria-label="Name"
        className="h-[19px] min-w-0 flex-1 border border-accent bg-vs-editor px-1 text-[13px] text-vs-text outline-none select-text"
      />
    </form>
  );
}

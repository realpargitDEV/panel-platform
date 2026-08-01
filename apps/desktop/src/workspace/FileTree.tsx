/**
 * The tree itself.
 *
 * One row per entry, indented by depth, recursing into expanded folders. Rows
 * are `div`s with tree roles rather than nested buttons: a row has to be a drop
 * target, a right-click target and a drag source at once, and a button that
 * contains other buttons is neither valid nor navigable.
 *
 * Rendering is deliberately not virtualised. The core caps a listing before it
 * returns it, so no single folder can be long enough to need it.
 */
import { useState } from 'react';

import type { FileEntry } from '../api';
import Icon from './Icon';
import { fileIconColor } from './fileIcons';
import { parentOf } from './tabs';

export interface TreeCallbacks {
  onOpen: (entry: FileEntry) => void;
  onToggle: (path: string) => void;
  onContextMenu: (entry: FileEntry, event: React.MouseEvent) => void;
  /** A file or folder dragged from inside the tree onto a folder row. */
  onMove: (from: string, toDirectory: string) => void;
  /** The folder a drop or a new file would land in. */
  onSelectDirectory: (path: string) => void;
  onSelectFile: (path: string) => void;
}

export interface TreeState {
  listings: Record<string, FileEntry[]>;
  expanded: string[];
  /** The row with the highlight — a file being edited or a folder just clicked. */
  selected: string | null;
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
  const isSelected = state.selected === entry.path;
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
    event.dataTransfer.setData('application/x-project-path', entry.path);
    event.dataTransfer.effectAllowed = 'move';
  }

  function isInternalDrag(event: React.DragEvent): boolean {
    return event.dataTransfer.types.includes('application/x-project-path');
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
    const from = event.dataTransfer.getData('application/x-project-path');
    if (from) callbacks.onMove(from, dropDirectory);
  }

  return (
    <div>
      <div
        role="treeitem"
        tabIndex={-1}
        aria-expanded={isDirectory ? isOpen : undefined}
        aria-selected={isSelected}
        aria-level={depth + 1}
        title={entry.kind === 'other' ? `${entry.path} — a link or special file` : entry.path}
        draggable
        onDragStart={onDragStart}
        onDragOver={onDragOver}
        onDragLeave={() => setDropping(false)}
        onDrop={onDrop}
        onContextMenu={(event) => callbacks.onContextMenu(entry, event)}
        onClick={() => {
          if (isDirectory) {
            callbacks.onSelectDirectory(entry.path);
            callbacks.onToggle(entry.path);
            return;
          }
          callbacks.onSelectFile(entry.path);
          if (entry.kind === 'file') callbacks.onOpen(entry);
        }}
        style={{ paddingLeft: `${ROW_PADDING + depth * INDENT}px` }}
        className={`group flex h-[22px] cursor-pointer items-center gap-1 pr-2 select-none ${
          isSelected
            ? 'bg-vs-active text-white'
            : isTarget
              ? 'bg-vs-selected text-vs-text'
              : 'text-vs-text hover:bg-white/5'
        } ${dropping ? 'vs-drop-target' : ''}`}
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

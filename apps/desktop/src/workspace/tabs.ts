/**
 * Open buffers, and which of them have unsaved changes.
 *
 * Kept as pure functions over a plain value rather than as `useState` calls
 * inside the view. The rules that are easy to get wrong — what happens when you
 * close the active dirty tab, what a rename does to an open buffer, whether a
 * save clears the marker for the right file — are all here, and all testable
 * without a window or an editor.
 */

export interface Buffer {
  /** Relative to the project root. The identity of a tab. */
  path: string;
  /** Last-saved text. What `dirty` is measured against. */
  saved: string;
  /** What is in the editor now. */
  current: string;
  /** Monaco language id, decided by the core from the extension. */
  language: string;
  /** True when the project is mid-build or mid-deletion. */
  readOnly: boolean;
}

export interface EditorState {
  buffers: Buffer[];
  /** The path of the visible tab, or null when nothing is open. */
  active: string | null;
}

export const emptyEditor: EditorState = { buffers: [], active: null };

export function isDirty(buffer: Buffer): boolean {
  return buffer.saved !== buffer.current;
}

export function dirtyPaths(state: EditorState): string[] {
  return state.buffers.filter(isDirty).map((buffer) => buffer.path);
}

export function activeBuffer(state: EditorState): Buffer | null {
  return state.buffers.find((buffer) => buffer.path === state.active) ?? null;
}

export function bufferFor(state: EditorState, path: string): Buffer | undefined {
  return state.buffers.find((buffer) => buffer.path === path);
}

/**
 * Open a file, or focus it if it is already open.
 *
 * An already-open buffer keeps its unsaved edits rather than being replaced by
 * what is on disk: clicking a file in the tree twice must not silently discard
 * typing.
 */
export function openFile(
  state: EditorState,
  file: { path: string; text: string; language: string; readOnly: boolean },
): EditorState {
  const existing = bufferFor(state, file.path);
  if (existing) {
    return { ...state, active: file.path };
  }
  return {
    buffers: [
      ...state.buffers,
      {
        path: file.path,
        saved: file.text,
        current: file.text,
        language: file.language,
        readOnly: file.readOnly,
      },
    ],
    active: file.path,
  };
}

/** Record a keystroke. */
export function edit(state: EditorState, path: string, text: string): EditorState {
  return {
    ...state,
    buffers: state.buffers.map((buffer) =>
      buffer.path === path ? { ...buffer, current: text } : buffer,
    ),
  };
}

/**
 * Mark a buffer saved.
 *
 * `saved` becomes the text that was written, not the text in the editor now: a
 * save is asynchronous, and typing during one must leave the buffer dirty
 * afterwards rather than pretending the newer characters reached the disk.
 */
export function markSaved(state: EditorState, path: string, writtenText: string): EditorState {
  return {
    ...state,
    buffers: state.buffers.map((buffer) =>
      buffer.path === path ? { ...buffer, saved: writtenText } : buffer,
    ),
  };
}

/**
 * Close a tab.
 *
 * Focus moves to the neighbour on the left, or the right when the closed tab was
 * first — the behaviour of every editor, and the one that does not dump the user
 * on an empty pane while other files are still open.
 */
export function closeFile(state: EditorState, path: string): EditorState {
  const index = state.buffers.findIndex((buffer) => buffer.path === path);
  if (index < 0) return state;

  const buffers = state.buffers.filter((buffer) => buffer.path !== path);
  if (state.active !== path) {
    return { ...state, buffers };
  }

  const neighbour = buffers[index - 1] ?? buffers[index] ?? null;
  return { buffers, active: neighbour ? neighbour.path : null };
}

/**
 * Follow a rename.
 *
 * The buffer keeps its unsaved text and changes identity. Without this, a rename
 * would leave a tab pointing at a path that no longer exists, and its next save
 * would recreate the old file.
 */
export function renamePath(state: EditorState, from: string, to: string): EditorState {
  return {
    buffers: state.buffers.map((buffer) =>
      buffer.path === from ? { ...buffer, path: to } : buffer,
    ),
    active: state.active === from ? to : state.active,
  };
}

/**
 * Forget buffers under a deleted path.
 *
 * Deleting a directory closes everything inside it, which is why this matches on
 * the prefix as well as the path itself.
 */
export function forgetDeleted(state: EditorState, path: string): EditorState {
  const gone = (candidate: string) => candidate === path || candidate.startsWith(`${path}/`);
  const buffers = state.buffers.filter((buffer) => !gone(buffer.path));
  const active =
    state.active !== null && gone(state.active) ? (buffers[0]?.path ?? null) : state.active;
  return { buffers, active };
}

/**
 * Close every tab except one, which becomes active.
 *
 * The kept tab is focused even if it was not before: "Close others" on an
 * inactive tab is a way of saying "this is the one I want".
 */
export function closeOthers(state: EditorState, path: string): EditorState {
  const kept = bufferFor(state, path);
  if (!kept) return state;
  return { buffers: [kept], active: path };
}

/**
 * Close everything to the right of one tab.
 *
 * Focus only moves when it was on a tab that is going away; a tab further left
 * keeps it.
 */
export function closeToRight(state: EditorState, path: string): EditorState {
  const index = state.buffers.findIndex((buffer) => buffer.path === path);
  if (index < 0) return state;

  const buffers = state.buffers.slice(0, index + 1);
  const stillOpen = buffers.some((buffer) => buffer.path === state.active);
  return { buffers, active: stillOpen ? state.active : path };
}

/** Close every tab. */
export function closeAll(): EditorState {
  return emptyEditor;
}

/** The tab label. Just the file name; the tooltip carries the path. */
export function tabLabel(path: string): string {
  const parts = path.split('/');
  return parts[parts.length - 1] || path;
}

/** Expand or collapse one directory in the tree. */
export function toggleExpanded(expanded: string[], path: string): string[] {
  return expanded.includes(path) ? expanded.filter((entry) => entry !== path) : [...expanded, path];
}

/** The parent directory of a path, or `''` for the project root. */
export function parentOf(path: string): string {
  const cut = path.lastIndexOf('/');
  return cut < 0 ? '' : path.slice(0, cut);
}

/** Join a directory and a name into a request path. */
export function childPath(directory: string, name: string): string {
  return directory ? `${directory}/${name}` : name;
}

import { childPath, parentOf } from './editor/tabs';

export interface DroppedFile {
  file: File;
  relativePath: string;
}

export interface DroppedItems {
  files: DroppedFile[];
  directories: string[];
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

export async function collectDroppedItems(dataTransfer: DataTransfer): Promise<DroppedItems> {
  const dropped: DroppedItems = { files: [], directories: [] };
  const items = Array.from(dataTransfer.items);

  if (items.length > 0) {
    for (const item of items) {
      if (item.kind !== 'file') continue;
      const entry = (item as DataTransferItemWithEntries).webkitGetAsEntry?.();
      if (entry) {
        mergeDroppedItems(dropped, await collectEntryItems(entry, ''));
        continue;
      }

      const file = item.getAsFile();
      if (file) dropped.files.push({ file, relativePath: file.webkitRelativePath || file.name });
    }
    if (dropped.files.length > 0 || dropped.directories.length > 0) return dropped;
  }

  return {
    files: Array.from(dataTransfer.files).map((file) => ({
      file,
      relativePath: file.webkitRelativePath || file.name,
    })),
    directories: [],
  };
}

/**
 * Should the webview's own drop event import what was dropped?
 *
 * A single drop can be delivered twice. Tauri's OS-level drag/drop event always
 * fires; on macOS and Linux the webview's HTML5 `drop` event fires for the same
 * gesture as well. (On Windows it never does — the webview only sees HTML5
 * drag/drop when `dragDropEnabled` is turned off, and it is on.)
 *
 * Deciding between the two on elapsed time means the same drop is imported
 * twice whenever the second delivery is late, and dropped entirely whenever an
 * unrelated drop follows too quickly. The rule here is a fact rather than a
 * timing guess: if the OS listener is live it owns every drop, and the HTML5
 * handler stands down. The HTML5 path is what runs when that listener could not
 * be registered — outside a Tauri webview, or if registration failed.
 */
export function shouldImportBrowserDrop(nativeListenerReady: boolean): boolean {
  return !nativeListenerReady;
}

/** A pointer position as Tauri reports it: physical device pixels. */
export interface DropPoint {
  x: number;
  y: number;
}

/** The part of a `DOMRect` a hit test needs, in CSS pixels. */
export interface DropZoneRect {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

/**
 * Is a native drag over the explorer?
 *
 * The OS-level drag/drop event Tauri delivers is a *window* event: it fires
 * wherever the pointer is, including over the editor and the page chrome.
 * Without this test a folder dropped anywhere in the window is imported into
 * the project, which is not what dropping onto an editor pane means.
 *
 * Tauri reports the position in physical device pixels while
 * `getBoundingClientRect` is in CSS pixels, so the point is scaled down before
 * being compared — on a 200% display the two disagree by a factor of two and an
 * unscaled comparison silently shrinks the drop zone to its top-left quarter.
 */
export function isInsideDropZone(
  position: DropPoint,
  rect: DropZoneRect,
  devicePixelRatio: number,
): boolean {
  const scale = devicePixelRatio > 0 ? devicePixelRatio : 1;
  const x = position.x / scale;
  const y = position.y / scale;
  return x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom;
}

export function cleanDropPath(path: string): string {
  return path
    .replace(/\\/g, '/')
    .split('/')
    .filter((part) => part.length > 0 && part !== '.')
    .join('/');
}

export function directoryPathsForDrop(files: DroppedFile[], directories: string[]): string[] {
  const paths = new Set<string>();
  const addWithParents = (path: string) => {
    let current = cleanDropPath(path);
    while (current) {
      paths.add(current);
      current = parentOf(current);
    }
  };

  for (const directory of directories) addWithParents(directory);
  for (const file of files) {
    const parent = parentOf(cleanDropPath(file.relativePath));
    if (parent) addWithParents(parent);
  }

  return [...paths].sort(
    (left, right) => pathDepth(left) - pathDepth(right) || left.localeCompare(right),
  );
}

export function duplicateDroppedFilePath(files: DroppedFile[]): string | null {
  const seen = new Set<string>();
  for (const file of files) {
    const path = cleanDropPath(file.relativePath);
    const key = path.toLocaleLowerCase();
    if (seen.has(key)) return path;
    seen.add(key);
  }
  return null;
}

async function collectEntryItems(
  entry: BrowserFileSystemEntry,
  prefix: string,
): Promise<DroppedItems> {
  const relativePath = childPath(prefix, entry.name);
  if (entry.isFile) {
    const file = await fileFromEntry(entry as BrowserFileSystemFileEntry);
    return { files: [{ file, relativePath }], directories: [] };
  }
  if (!entry.isDirectory) return { files: [], directories: [] };

  const childEntries = await readAllEntries(entry as BrowserFileSystemDirectoryEntry);
  const dropped: DroppedItems = { files: [], directories: [relativePath] };
  for (const child of childEntries) {
    mergeDroppedItems(dropped, await collectEntryItems(child, relativePath));
  }
  return dropped;
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

function mergeDroppedItems(target: DroppedItems, source: DroppedItems) {
  target.files.push(...source.files);
  target.directories.push(...source.directories);
}

function pathDepth(path: string): number {
  return path.split('/').length;
}

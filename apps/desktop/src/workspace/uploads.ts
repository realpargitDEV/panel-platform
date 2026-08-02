/**
 * What a transfer is, while it is happening.
 *
 * Two kinds, because there are two ways files arrive. A *browser* upload is a
 * `File` the webview handed us, sent to the core in chunks. A *native* import
 * is a list of paths the operating system named, copied by the core itself —
 * the only way a whole folder can arrive without the webview reading every byte
 * of it into memory first.
 *
 * The types and the small formatting rules live here; the machinery that runs
 * them lives in the workspace, which is where the API calls are.
 */

export type UploadStatus = 'queued' | 'uploading' | 'success' | 'failed' | 'cancelled';

interface BaseUploadItem {
  id: string;
  uploadId: string;
  /** Where it is going, relative to the project root. */
  path: string;
  uploadedBytes: number;
  sizeBytes: number;
  status: UploadStatus;
  message: string;
  copiedFiles?: number;
  totalFiles?: number;
  /** Set when this transfer is replacing an entry that had to be deleted first. */
  replaces?: boolean;
}

export interface BrowserUploadItem extends BaseUploadItem {
  kind: 'browser';
  file: File;
}

export interface NativeImportItem extends BaseUploadItem {
  kind: 'native';
  sourcePaths: string[];
  targetDirectory: string;
  /**
   * The subset of `sourcePaths` whose contents land in the target rather than
   * the folder itself — a project dropped into a project.
   */
  unwrapPaths?: string[];
}

export type UploadItem = BrowserUploadItem | NativeImportItem;
export type UploadPatch = Partial<BaseUploadItem>;

/** How much of a file is sent per `append` call. */
export const UPLOAD_CHUNK_BYTES = 512 * 1024;

/** The rejection used to unwind an upload the user cancelled. */
export const UPLOAD_CANCELLED = 'upload-cancelled';

export function createUploadId(): string {
  if (typeof crypto.randomUUID === 'function') return crypto.randomUUID();
  return `${Date.now().toString(36)}-${Math.random().toString(36).slice(2)}`;
}

/** A folder path as a person reads it. The project root has no name of its own. */
export function displayDirectory(path: string): string {
  return path ? path : 'the project root';
}

/** The last component of a path the operating system gave us, in either slash. */
export function fileNameFromNativePath(path: string): string {
  const normalised = path.replace(/\\/g, '/').replace(/\/+$/, '');
  return normalised.split('/').pop() || path;
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)} KB`;
  if (bytes < 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  return `${(bytes / (1024 * 1024 * 1024)).toFixed(1)} GB`;
}

/**
 * What a native import is called in the transfer list.
 *
 * One item gets its own name; several get a count, because listing forty paths
 * in a 200px sidebar tells nobody anything.
 */
export function nativeImportLabel(paths: string[], targetDirectory: string): string {
  const [first] = paths;
  if (paths.length === 1 && first) {
    const name = fileNameFromNativePath(first);
    return targetDirectory ? `${targetDirectory}/${name}` : name;
  }
  return `${paths.length} items → ${displayDirectory(targetDirectory)}`;
}

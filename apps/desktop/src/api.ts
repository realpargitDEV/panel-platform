/**
 * The typed edge between the window and the Rust core.
 *
 * Every call to `invoke` in the application goes through this file. Keeping
 * them in one place is what makes it possible to see, in one screen, the
 * complete list of things the interface can ask the core to do.
 */
import { invoke } from '@tauri-apps/api/core';

export interface SystemStatus {
  appVersion: string;
  schemaVersion: number;
  uptimeSeconds: number;
  startedAt: string;
  dockerAvailable: boolean;
  dockerSummary: string;
  dockerVersion: string | null;
  dockerHint: string | null;
}

export interface ProjectSummary {
  id: string;
  slug: string;
  displayName: string;
  description: string;
  projectType: string;
  status: string;
  desiredState: string;
  color: string | null;
}

export interface AvailableUpdate {
  currentVersion: string;
  newVersion: string;
  notes: string;
  publishedAt: string | null;
  downloadUrl: string;
  signature: string;
}

export type UpdateCheck =
  | { state: 'up_to_date'; currentVersion: string }
  | ({ state: 'available' } & AvailableUpdate)
  | { state: 'skipped'; skippedVersion: string }
  | { state: 'ahead_of_published'; currentVersion: string; publishedVersion: string };

/**
 * Rust serialises with snake_case field names; the window reads camelCase.
 * Converting once, here, beats spelling `docker_available` throughout the
 * components — and beats configuring serde to rename, which would change the
 * shape of every stored document too.
 */
function toCamel<T>(value: unknown): T {
  if (Array.isArray(value)) {
    return value.map((item) => toCamel(item)) as unknown as T;
  }
  if (value !== null && typeof value === 'object') {
    const out: Record<string, unknown> = {};
    for (const [key, inner] of Object.entries(value)) {
      const camel = key.replace(/_([a-z])/g, (_, letter: string) => letter.toUpperCase());
      out[camel] = toCamel(inner);
    }
    return out as T;
  }
  return value as T;
}

/**
 * A failed command arrives as `{ message }`. Anything else means the bridge
 * itself broke, which is worth showing differently from an expected failure.
 */
export function errorMessage(error: unknown): string {
  if (error !== null && typeof error === 'object' && 'message' in error) {
    return String((error as { message: unknown }).message);
  }
  return String(error);
}

export async function systemStatus(): Promise<SystemStatus> {
  return toCamel<SystemStatus>(await invoke('system_status'));
}

export async function listProjects(): Promise<ProjectSummary[]> {
  return toCamel<ProjectSummary[]>(await invoke('list_projects'));
}

export async function checkForUpdate(): Promise<UpdateCheck> {
  return toCamel<UpdateCheck>(await invoke('check_for_update'));
}

/**
 * Download the offered update, verify its signature, and install it.
 *
 * Resolves only on Linux, where the AppImage is replaced in place and the new
 * version appears on the next start. On Windows the installer takes over and
 * this process exits, so nothing here runs afterwards — the caller must not
 * depend on code after the await.
 */
export async function installUpdate(): Promise<void> {
  await invoke('install_update');
}

/** Where a new project's files come from. */
export type SourceKind = 'EMPTY' | 'GIT_CLONE' | 'REMOTE_ARCHIVE' | 'GITHUB_CLI';

export interface ProjectSource {
  kind: SourceKind;
  /**
   * An `https://` address — or, for `GITHUB_CLI`, an `owner/repo` name. Both are
   * validated by the core, not here.
   */
  url?: string;
  /** Branch or tag. A commit id is refused with an explanation. */
  gitRef?: string;
  subdirectory?: string;
  /**
   * A token for a private remote.
   *
   * Deliberately never read back: no function in this file returns one, and the
   * core has no command that would. It travels one way.
   */
  token?: string;
}

export interface NewProjectRequest {
  displayName: string;
  description: string;
  /**
   * Leave undefined to let the core look at the files and decide. An empty
   * project has no files, so it needs one.
   */
  runtime?: string;
  source?: ProjectSource;
}

/** What a project turned out to be, once its files were there to look at. */
export interface CreatedProject extends ProjectSummary {
  runtime: string;
  /** True when the runtime came from the files rather than from a choice. */
  detected: boolean;
  /** Every language found in the tree. */
  languages: string[];
  /** Detection warnings, already phrased for a person. */
  notes: string[];
}

export interface GitHubCliStatus {
  installed: boolean;
  /** The logged-in account. Null with `installed: true` means nobody is. */
  account: string | null;
  /** What the user needs to do, when something needs doing. */
  hint: string | null;
}

/** Asked before the GitHub CLI option is offered. */
export async function githubCliStatus(): Promise<GitHubCliStatus> {
  return toCamel<GitHubCliStatus>(await invoke('github_cli_status'));
}

export interface RuntimeOption {
  id: string;
  label: string;
}

/** The override list, served from the same table the planner uses. */
export async function supportedRuntimes(): Promise<RuntimeOption[]> {
  return toCamel<RuntimeOption[]>(await invoke('supported_runtimes'));
}

export async function createProject(request: NewProjectRequest): Promise<CreatedProject> {
  // The command reads snake_case, so the one place that converts back is here
  // rather than in the form.
  return toCamel<CreatedProject>(
    await invoke('create_project', {
      request: {
        display_name: request.displayName,
        description: request.description,
        runtime: request.runtime,
        source: request.source
          ? {
              kind: request.source.kind,
              url: request.source.url,
              git_ref: request.source.gitRef,
              subdirectory: request.source.subdirectory,
              token: request.source.token,
            }
          : undefined,
      },
    }),
  );
}

export async function startProject(projectId: string): Promise<string> {
  return invoke('start_project', { projectId });
}

export async function stopProject(projectId: string): Promise<void> {
  return invoke('stop_project', { projectId });
}

export async function restartProject(projectId: string): Promise<string> {
  return invoke('restart_project', { projectId });
}

// ------------------------------------------------------------- project files

export interface FileEntry {
  name: string;
  /** Relative to the project root, forward-slashed on every platform. */
  path: string;
  kind: 'file' | 'directory' | 'other';
  sizeBytes: number;
  modifiedUnixMs: number | null;
  isSymlink: boolean;
}

export interface Listing {
  path: string;
  entries: FileEntry[];
  /** True when there were more entries than the core will return at once. */
  truncated: boolean;
}

export interface TextFile {
  path: string;
  text: string;
  sizeBytes: number;
  /** The Monaco language id, decided by the core from the extension. */
  language: string;
  /** True while the project is being built or removed. */
  readOnly: boolean;
}

/**
 * Every path below is *relative to the project root*, and the core builds the
 * real path itself. There is deliberately no way to send an absolute path: the
 * editor is the first feature with a reason to want one, and it does not get it.
 */
export async function listProjectFiles(projectId: string, path: string): Promise<Listing> {
  return toCamel<Listing>(await invoke('list_project_files', { projectId, path }));
}

export async function readProjectFile(projectId: string, path: string): Promise<TextFile> {
  return toCamel<TextFile>(await invoke('read_project_file', { projectId, path }));
}

export async function writeProjectFile(
  projectId: string,
  path: string,
  text: string,
): Promise<FileEntry> {
  return toCamel<FileEntry>(await invoke('write_project_file', { projectId, path, text }));
}

export async function createProjectFile(
  projectId: string,
  path: string,
  directory: boolean,
): Promise<FileEntry> {
  return toCamel<FileEntry>(await invoke('create_project_file', { projectId, path, directory }));
}

export async function renameProjectFile(
  projectId: string,
  path: string,
  newName: string,
): Promise<FileEntry> {
  return toCamel<FileEntry>(await invoke('rename_project_file', { projectId, path, newName }));
}

export async function deleteProjectFile(
  projectId: string,
  path: string,
  recursive: boolean,
): Promise<void> {
  return invoke('delete_project_file', { projectId, path, recursive });
}

export async function searchProjectFiles(projectId: string, query: string): Promise<FileEntry[]> {
  return toCamel<FileEntry[]>(await invoke('search_project_files', { projectId, query }));
}

export interface AppSettings {
  mode: string;
  logLevel: string;
  logJson: boolean;
  logRetentionDays: number;
  maxProjects: number;
  maxUploadBytes: number;
  portPoolStart: number;
  portPoolEnd: number;
  portPoolSize: number;
  dockerEnabled: boolean;
  dataDir: string;
  projectsDir: string;
  logsDir: string;
  backupsDir: string;
}

export async function appSettings(): Promise<AppSettings> {
  return toCamel<AppSettings>(await invoke('app_settings'));
}

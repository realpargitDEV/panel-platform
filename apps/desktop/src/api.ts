/**
 * The typed edge between the window and the Rust core.
 *
 * Every call to `invoke` in the application goes through this file. Keeping
 * them in one place is what makes it possible to see, in one screen, the
 * complete list of things the interface can ask the core to do.
 */
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';

import type { UpdateProgressEvent } from './update';

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

/**
 * Subscribe to install progress.
 *
 * The core emits `update://progress` as bytes arrive, when it starts verifying
 * the signature, and when the installer takes over. Resolves to the function
 * that stops listening.
 */
export async function onUpdateProgress(
  handler: (progress: UpdateProgressEvent) => void,
): Promise<() => void> {
  return listen<UpdateProgressEvent>('update://progress', (event) => handler(event.payload));
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

export async function killProject(projectId: string): Promise<void> {
  return invoke('kill_project', { projectId });
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

export interface FileImportProgressEvent {
  importId: string;
  projectId: string;
  copiedBytes: number;
  totalBytes: number;
  copiedFiles: number;
  totalFiles: number;
  currentPath: string;
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

export async function beginProjectFileUpload(
  projectId: string,
  path: string,
  uploadId: string,
  totalSize: number,
): Promise<void> {
  return invoke('begin_project_file_upload', { projectId, path, uploadId, totalSize });
}

export async function appendProjectFileUpload(
  projectId: string,
  path: string,
  uploadId: string,
  offset: number,
  bytes: number[],
): Promise<number> {
  return invoke('append_project_file_upload', { projectId, path, uploadId, offset, bytes });
}

export async function finishProjectFileUpload(
  projectId: string,
  path: string,
  uploadId: string,
  totalSize: number,
): Promise<FileEntry> {
  return toCamel<FileEntry>(
    await invoke('finish_project_file_upload', { projectId, path, uploadId, totalSize }),
  );
}

export async function cancelProjectFileUpload(
  projectId: string,
  path: string,
  uploadId: string,
): Promise<void> {
  return invoke('cancel_project_file_upload', { projectId, path, uploadId });
}

/**
 * Copy paths from this machine into the project.
 *
 * `unwrapPaths` is the subset of `sourcePaths` whose *contents* should land in
 * the target rather than the folder itself — a project dropped into a project,
 * where keeping the folder would produce `MyProject/MyProject/package.json`.
 */
export async function importProjectFiles(
  projectId: string,
  targetDirectory: string,
  sourcePaths: string[],
  importId: string,
  unwrapPaths: string[] = [],
  /** `[absolute source, final name]` for anything a resolution renamed. */
  destinationNames: [string, string][] = [],
): Promise<FileEntry[]> {
  return toCamel<FileEntry[]>(
    await invoke('import_project_files', {
      projectId,
      targetDirectory,
      sourcePaths,
      unwrapPaths,
      destinationNames,
      importId,
    }),
  );
}

/**
 * What a set of dropped paths turns out to be.
 *
 * The window has no filesystem access — the drag-and-drop event hands it
 * operating-system paths and nothing else — so the core looks and reports, and
 * the window decides what to offer.
 */
export interface ImportCandidate {
  path: string;
  name: string;
  isDirectory: boolean;
  /** True when the evidence says this folder is a project in its own right. */
  isProject: boolean;
  /** The score behind that decision, so the interface can explain itself. */
  score: number;
  /** The markers found, e.g. `package.json`, `src/`. */
  signals: string[];
  /** Top-level names inside, capped for the preview. */
  children: string[];
  childCount: number;
  /** `Node.js`, `Rust`, `Tauri`… null when nothing identified it. */
  ecosystem: string | null;
  /** True when the folder holds several packages rather than being one. */
  isMonorepo: boolean;
  /** Projects found inside this one. */
  nested: NestedProject[];
}

/**
 * A project found inside another.
 *
 * `belongsToWorkspace` is the difference between a monorepo's member — which
 * must stay with its parent — and two unrelated projects that happened to be
 * dropped in one folder.
 */
export interface NestedProject {
  path: string;
  relative: string;
  name: string;
  ecosystem: string | null;
  score: number;
  belongsToWorkspace: boolean;
}

export async function inspectImportPaths(sourcePaths: string[]): Promise<ImportCandidate[]> {
  return toCamel<ImportCandidate[]>(await invoke('inspect_import_paths', { sourcePaths }));
}

/**
 * One thing an import would create.
 *
 * The window cannot work these out: unwrapping a folder lands its children and
 * the window cannot read a directory. The sizes come back in the same call, so
 * one walk answers both "what will collide?" and "how much is there?".
 */
export interface PlannedDestination {
  /** The absolute source — a child, when a folder is unwrapped. */
  source: string;
  /** Where it lands, relative to the project root. */
  relative: string;
  isDirectory: boolean;
  totalFiles: number;
  totalBytes: number;
  /** `file`, `directory`, or null when the path is free. */
  existing: string | null;
}

export async function planImportDestinations(
  projectId: string,
  targetDirectory: string,
  sourcePaths: string[],
  unwrapPaths: string[] = [],
): Promise<PlannedDestination[]> {
  return toCamel<PlannedDestination[]>(
    await invoke('plan_import_destinations', {
      projectId,
      targetDirectory,
      sourcePaths,
      unwrapPaths,
    }),
  );
}

export async function cancelProjectFileImport(importId: string): Promise<void> {
  return invoke('cancel_project_file_import', { importId });
}

export async function onFileImportProgress(
  handler: (progress: FileImportProgressEvent) => void,
): Promise<() => void> {
  return listen<FileImportProgressEvent>('project-files://import-progress', (event) =>
    handler(event.payload),
  );
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

/**
 * Move an entry somewhere else in the same project.
 *
 * Separate from {@link renameProjectFile} because that one takes a single name
 * and refuses anything with a separator in it, so a rename cannot relocate a
 * file by accident. This is what the explorer's drag-and-drop uses.
 */
export async function moveProjectFile(
  projectId: string,
  from: string,
  to: string,
): Promise<FileEntry> {
  return toCamel<FileEntry>(await invoke('move_project_file', { projectId, from, to }));
}

/** Copy an entry within the project — the explorer's "Duplicate". */
export async function copyProjectFile(
  projectId: string,
  from: string,
  to: string,
): Promise<FileEntry> {
  return toCamel<FileEntry>(await invoke('copy_project_file', { projectId, from, to }));
}

export async function searchProjectFiles(projectId: string, query: string): Promise<FileEntry[]> {
  return toCamel<FileEntry[]>(await invoke('search_project_files', { projectId, query }));
}

/**
 * The project's folder on this machine.
 *
 * The only absolute path the window ever sees, and it is used for display only:
 * every command still takes a path relative to the root.
 */
export async function projectRootPath(projectId: string): Promise<string> {
  return invoke('project_root_path', { projectId });
}

/** Show one file or folder in the system's file manager. */
export async function revealProjectPath(projectId: string, path: string): Promise<void> {
  return invoke('reveal_project_path', { projectId, path });
}

// ------------------------------------------------------------ machine health

/**
 * What this machine is doing, measured.
 *
 * Host-wide rather than per project: reading a container's own CPU and memory
 * means Docker's stats stream, which the manager does not do yet. These numbers
 * are real, which is why they are worth showing at all.
 */
export interface SystemMetrics {
  cpuPercent: number;
  cpuCount: number;
  memoryUsedBytes: number;
  memoryTotalBytes: number;
  diskUsedBytes: number;
  diskTotalBytes: number;
  diskMount: string;
}

export async function systemMetrics(): Promise<SystemMetrics> {
  return toCamel<SystemMetrics>(await invoke('system_metrics'));
}

/** One line of the audit log the core already writes. */
export interface ActivityEntry {
  id: string;
  occurredAt: string;
  action: string;
  result: string;
  targetType: string | null;
  targetId: string | null;
  targetLabel: string | null;
  errorCode: string | null;
}

/** Newest first. An empty list means nothing has happened, not that it failed. */
export async function recentActivity(limit: number, projectId?: string): Promise<ActivityEntry[]> {
  return toCamel<ActivityEntry[]>(await invoke('recent_activity', { projectId, limit }));
}

// ----------------------------------------------------------- project details

export interface RuntimeDetail {
  runtime: string;
  runtimeVersion: string;
  packageManager: string;
  installCommand: string | null;
  buildCommand: string | null;
  startCommand: string;
  workingDir: string;
  entryFile: string | null;
  healthCheckType: string;
  healthCheckTarget: string | null;
}

export interface PortMapping {
  containerPort: number;
  hostPort: number | null;
  protocol: string;
}

/**
 * An environment variable. A secret's value is never sent — the screen shows
 * that the key exists, not what it holds.
 */
export interface EnvVarSummary {
  key: string;
  isSecret: boolean;
  restartRequired: boolean;
  value: string | null;
}

export interface ProjectDetail {
  id: string;
  slug: string;
  displayName: string;
  description: string;
  projectType: string;
  status: string;
  desiredState: string;
  health: string;
  runMode: string;
  restartPolicy: string;
  networkMode: string;
  autostart: boolean;
  directory: string;
  sourceType: string;
  sourceUrl: string | null;
  sourceRef: string | null;
  sourceCommit: string | null;
  imageTag: string | null;
  containerName: string | null;
  memoryLimitMb: number;
  cpuLimitCores: number;
  storageLimitMb: number;
  startedAt: string | null;
  stoppedAt: string | null;
  lastExitCode: number | null;
  lastFailureAt: string | null;
  lastFailureReason: string | null;
  restartCount: number;
  createdAt: string;
  updatedAt: string;
  runtime: RuntimeDetail | null;
  ports: PortMapping[];
  envVars: EnvVarSummary[];
}

export async function projectDetails(projectId: string): Promise<ProjectDetail> {
  return toCamel<ProjectDetail>(await invoke('project_details', { projectId }));
}

export interface DeploymentSummary {
  id: string;
  deploymentType: string;
  status: string;
  imageTag: string | null;
  errorCode: string | null;
  errorMessage: string | null;
  startedAt: string;
  finishedAt: string | null;
  durationMs: number | null;
}

export async function projectDeployments(
  projectId: string,
  limit: number,
): Promise<DeploymentSummary[]> {
  return toCamel<DeploymentSummary[]>(await invoke('project_deployments', { projectId, limit }));
}

/** Starts, stops and crashes — the restart history. */
export interface ContainerEvent {
  id: string;
  eventType: string;
  exitCode: number | null;
  detail: string | null;
  occurredAt: string;
}

export async function projectEvents(projectId: string, limit: number): Promise<ContainerEvent[]> {
  return toCamel<ContainerEvent[]>(await invoke('project_events', { projectId, limit }));
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

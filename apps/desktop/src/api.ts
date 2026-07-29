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

export interface NewProjectRequest {
  displayName: string;
  description: string;
  runtime: string;
}

export async function createProject(request: NewProjectRequest): Promise<ProjectSummary> {
  // The command reads snake_case, so the one place that converts back is here
  // rather than in the form.
  return toCamel<ProjectSummary>(
    await invoke('create_project', {
      request: {
        display_name: request.displayName,
        description: request.description,
        runtime: request.runtime,
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

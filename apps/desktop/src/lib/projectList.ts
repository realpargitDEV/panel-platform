/**
 * Narrowing and ordering the project list.
 *
 * Pure, and tested, because filtering plus sorting is where a list quietly
 * starts lying: a filter that also reorders, a sort that puts the failed
 * projects last, a search that misses the one project whose name differs only
 * in case.
 */
import type { ProjectSummary } from '../api';
import { isFailed, isRunning, statusLook } from './projects';

export type StatusFilter = 'all' | 'running' | 'stopped' | 'failed';
export type SortKey = 'name' | 'status' | 'runtime';
export type ViewMode = 'grid' | 'list';

export interface ListOptions {
  query: string;
  status: StatusFilter;
  /** A runtime id such as `NODEJS`, or `all`. */
  runtime: string;
  sort: SortKey;
}

export const defaultListOptions: ListOptions = {
  query: '',
  status: 'all',
  runtime: 'all',
  sort: 'name',
};

/** Every runtime present in the list, for the filter's options. */
export function runtimesIn(projects: ProjectSummary[]): string[] {
  return [...new Set(projects.map((project) => project.projectType))].sort((left, right) =>
    left.localeCompare(right),
  );
}

function matchesQuery(project: ProjectSummary, needle: string): boolean {
  if (needle.length === 0) return true;
  return (
    project.displayName.toLowerCase().includes(needle) ||
    project.slug.toLowerCase().includes(needle) ||
    project.description.toLowerCase().includes(needle)
  );
}

function matchesStatus(project: ProjectSummary, filter: StatusFilter): boolean {
  switch (filter) {
    case 'running':
      return isRunning(project.status);
    case 'failed':
      return isFailed(project.status);
    case 'stopped':
      return !isRunning(project.status) && !isFailed(project.status);
    default:
      return true;
  }
}

/**
 * The order statuses sort in when sorting by status.
 *
 * Failed first: the point of sorting by status is to find what is wrong, and a
 * list that buries the broken project at the bottom has not answered the
 * question that was asked.
 */
function statusRank(status: string): number {
  if (isFailed(status)) return 0;
  if (statusLook(status).transitioning) return 1;
  if (isRunning(status)) return 2;
  return 3;
}

export function applyListOptions(
  projects: ProjectSummary[],
  options: ListOptions,
): ProjectSummary[] {
  const needle = options.query.trim().toLowerCase();

  const filtered = projects.filter(
    (project) =>
      matchesQuery(project, needle) &&
      matchesStatus(project, options.status) &&
      (options.runtime === 'all' || project.projectType === options.runtime),
  );

  const byName = (left: ProjectSummary, right: ProjectSummary) =>
    left.displayName.localeCompare(right.displayName, undefined, { sensitivity: 'base' });

  // Sorted on a copy: `filter` already made one, but a caller passing an
  // unfiltered array would otherwise have its own list reordered underneath it.
  return [...filtered].sort((left, right) => {
    switch (options.sort) {
      case 'status':
        return statusRank(left.status) - statusRank(right.status) || byName(left, right);
      case 'runtime':
        return left.projectType.localeCompare(right.projectType) || byName(left, right);
      default:
        return byName(left, right);
    }
  });
}

/** True when a filter is hiding something, so the empty state can say so. */
export function isFiltered(options: ListOptions): boolean {
  return options.query.trim().length > 0 || options.status !== 'all' || options.runtime !== 'all';
}

/** `NODEJS` reads as `Node.js`. Falls back to the id rather than hiding it. */
export function runtimeLabel(id: string): string {
  const known: Record<string, string> = {
    NODEJS: 'Node.js',
    TYPESCRIPT: 'TypeScript',
    BUN: 'Bun',
    DENO: 'Deno',
    PYTHON: 'Python',
    GO: 'Go',
    RUST: 'Rust',
    JAVA: 'Java',
    PHP: 'PHP',
    RUBY: 'Ruby',
    DOTNET: '.NET',
    STATIC: 'Static site',
    STATIC_SITE: 'Static site',
    DOCKERFILE: 'Dockerfile',
    DOCKER_COMPOSE: 'Docker Compose',
  };
  const upper = id.toUpperCase();
  if (known[upper]) return known[upper];
  const lower = id.toLowerCase().replace(/_/g, ' ');
  return lower.charAt(0).toUpperCase() + lower.slice(1);
}

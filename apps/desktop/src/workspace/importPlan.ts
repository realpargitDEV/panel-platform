/**
 * What to do with what was dropped.
 *
 * The core reports what each dropped path *is*; this decides where it should
 * land. The rule that matters: a folder that is a project is a container the
 * user has already opened, so its contents belong at the destination and the
 * folder itself does not. Keeping it produces `MyProject/MyProject/package.json`,
 * which is the bug this exists to fix.
 *
 * Pure, and tested, because the failure mode is silent: an import that puts
 * files one level too deep still succeeds, still reports success, and is only
 * noticed later when nothing runs.
 */
import type { ImportCandidate } from '../api';

export interface ImportPlan {
  /** Every path to import, in the order they were dropped. */
  sourcePaths: string[];
  /** The subset whose contents land in the target rather than the folder. */
  unwrapPaths: string[];
  /** Folders judged to be projects in their own right. */
  projects: ImportCandidate[];
  /** Folders that are just folders. */
  folders: ImportCandidate[];
  files: ImportCandidate[];
  /**
   * True when the plan will unwrap something, so the interface knows to ask
   * before doing it. Unwrapping is the surprising outcome and the one worth a
   * confirmation; keeping a folder is what dropping a folder normally means.
   */
  unwraps: boolean;
}

/**
 * Decide the import.
 *
 * Exactly one project is unwrapped. Two or more are each kept in their own
 * folder — merging several projects into one root would mix their files, and
 * nothing about a multi-project drop says which one is "the" project.
 */
export function planImport(candidates: ImportCandidate[]): ImportPlan {
  const projects = candidates.filter((candidate) => candidate.isDirectory && candidate.isProject);
  const folders = candidates.filter((candidate) => candidate.isDirectory && !candidate.isProject);
  const files = candidates.filter((candidate) => !candidate.isDirectory);

  // One project: its contents are what the user means by "import this".
  // Several: keep them apart, because there is no answer to which one wins.
  const unwrapPaths = projects.length === 1 ? [projects[0]!.path] : [];

  return {
    sourcePaths: candidates.map((candidate) => candidate.path),
    unwrapPaths,
    projects,
    folders,
    files,
    unwraps: unwrapPaths.length > 0,
  };
}

/** The sentence shown once the import is queued. */
export function describePlan(plan: ImportPlan, targetDirectory: string): string {
  const where = targetDirectory ? targetDirectory : 'the project root';
  const parts: string[] = [];

  if (plan.unwraps) {
    const project = plan.projects[0];
    parts.push(`the contents of ${project?.name ?? 'the project'}`);
  } else if (plan.projects.length > 0) {
    parts.push(count(plan.projects.length, 'project'));
  }
  if (plan.folders.length > 0) parts.push(count(plan.folders.length, 'folder'));
  if (plan.files.length > 0) parts.push(count(plan.files.length, 'file'));

  if (parts.length === 0) return `Importing into ${where}.`;
  return `Importing ${asList(parts)} into ${where}.`;
}

/**
 * Why a folder was treated as a project, in words.
 *
 * The interface promises to say why rather than only what, so a wrong decision
 * can be argued with instead of only worked around.
 */
export function explainDetection(candidate: ImportCandidate): string {
  if (!candidate.isDirectory) return 'A file.';
  if (candidate.signals.length === 0) {
    return 'No project markers were found, so it is imported as an ordinary folder.';
  }
  const found = candidate.signals.slice(0, 6).join(', ');
  const more = candidate.signals.length > 6 ? `, and ${candidate.signals.length - 6} more` : '';
  return candidate.isProject
    ? `Detected as a project: ${found}${more}.`
    : `Some markers were found (${found}${more}) but not enough to call it a project.`;
}

function count(value: number, noun: string): string {
  return `${value} ${noun}${value === 1 ? '' : 's'}`;
}

function asList(parts: string[]): string {
  if (parts.length === 1) return parts[0]!;
  return `${parts.slice(0, -1).join(', ')} and ${parts[parts.length - 1]}`;
}

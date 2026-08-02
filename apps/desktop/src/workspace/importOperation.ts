/**
 * One organised import, as a single operation.
 *
 * The core imports into one directory at a time, so an organised import is
 * several calls. Reporting them as several unrelated transfers is what the
 * previous version did, and it meant a bar that filled up and reset three
 * times with no way to tell how much was left overall.
 *
 * This aggregates them: totals are measured before the first byte moves, and
 * every batch's progress adds to what the batches before it already finished.
 * The reduction is pure and tested, because "monotonic" is the property that
 * quietly breaks and cannot be seen in a screenshot.
 */

/**
 * The states an organised import can actually be in.
 *
 * There is deliberately no `moving-sources`: an organised import *copies* from
 * outside the project and never moves the originals, so nothing could ever emit
 * it. A phase no code can reach is worse than a missing one — it survives in
 * exhaustive switches and in tests as evidence of behaviour that does not
 * exist. Move operations are `executeResolved`, which is not this state machine.
 */
export type ImportPhase =
  | 'validating'
  | 'preparing'
  | 'copying'
  | 'staging-replacements'
  | 'committing'
  | 'finalising'
  | 'cancelling'
  | 'rolling-back'
  | 'completed'
  | 'failed';

export interface ImportOperationError {
  path: string;
  message: string;
  /** True when this is content the rollback could not put back. */
  rollback?: boolean;
}

export interface ImportOperationProgress {
  operationId: string;
  phase: ImportPhase;

  totalGroups: number;
  currentGroupIndex: number;
  currentGroupName: string | null;

  totalEntries: number;
  processedEntries: number;

  /** Null while the size is genuinely unknown, which shows as indeterminate. */
  totalBytes: number | null;
  processedBytes: number;

  currentSourcePath: string | null;
  currentDestinationPath: string | null;

  canCancel: boolean;
  cancellationRequested: boolean;

  rollbackTotal: number;
  rollbackProcessed: number;

  errors: ImportOperationError[];
}

/** One call the operation will make, with what it is expected to cost. */
export interface OperationBatch {
  /** The import id the core reports progress under. */
  importId: string;
  groupName: string;
  destination: string;
  sourcePaths: string[];
  unwrapPaths: string[];
  /** `(absolute source, final name)` for anything renamed by a resolution. */
  destinationNames: [string, string][];
  /** Sources whose existing destination is staged aside first. */
  replacePaths: string[];
  totalEntries: number;
  totalBytes: number;
}

export function newOperation(
  operationId: string,
  batches: OperationBatch[],
): ImportOperationProgress {
  const totalBytes = batches.reduce((sum, batch) => sum + batch.totalBytes, 0);
  return {
    operationId,
    phase: 'validating',
    totalGroups: batches.length,
    currentGroupIndex: 0,
    currentGroupName: batches[0]?.groupName ?? null,
    totalEntries: batches.reduce((sum, batch) => sum + batch.totalEntries, 0),
    processedEntries: 0,
    // Zero is a real total for an import of empty folders; unknown is not the
    // same thing, and the bar has to tell them apart.
    totalBytes: batches.length === 0 ? null : totalBytes,
    processedBytes: 0,
    currentSourcePath: null,
    currentDestinationPath: null,
    canCancel: true,
    cancellationRequested: false,
    rollbackTotal: 0,
    rollbackProcessed: 0,
    errors: [],
  };
}

/** What the core reports while one batch copies. */
export interface BatchProgress {
  importId: string;
  copiedFiles: number;
  copiedBytes: number;
  totalFiles: number;
  totalBytes: number;
  currentPath: string;
}

/**
 * Fold one batch's progress into the operation.
 *
 * `finished` is what the batches *before* this one already contributed, so the
 * total never dips when a new batch starts from zero. Values are clamped
 * upward as well: a batch that reports fewer bytes than it did a moment ago —
 * which happens when the core re-measures — must not drag the bar backwards.
 */
export function applyBatchProgress(
  operation: ImportOperationProgress,
  batches: OperationBatch[],
  finished: { entries: number; bytes: number },
  event: BatchProgress,
): ImportOperationProgress {
  const index = batches.findIndex((batch) => batch.importId === event.importId);
  if (index < 0) return operation;

  const batch = batches[index];
  const processedEntries = Math.max(
    operation.processedEntries,
    finished.entries + event.copiedFiles,
  );
  const processedBytes = Math.max(operation.processedBytes, finished.bytes + event.copiedBytes);

  return {
    ...operation,
    phase: operation.cancellationRequested ? 'cancelling' : 'copying',
    currentGroupIndex: index,
    currentGroupName: batch?.groupName ?? operation.currentGroupName,
    processedEntries,
    processedBytes,
    currentSourcePath: event.currentPath || operation.currentSourcePath,
    currentDestinationPath: batch?.destination ?? operation.currentDestinationPath,
  };
}

/** Move to the next batch, crediting everything the finished one carried. */
export function completeBatch(
  operation: ImportOperationProgress,
  batches: OperationBatch[],
  importId: string,
): ImportOperationProgress {
  const index = batches.findIndex((batch) => batch.importId === importId);
  if (index < 0) return operation;

  const done = batches.slice(0, index + 1);
  return {
    ...operation,
    processedEntries: Math.max(
      operation.processedEntries,
      done.reduce((sum, batch) => sum + batch.totalEntries, 0),
    ),
    processedBytes: Math.max(
      operation.processedBytes,
      done.reduce((sum, batch) => sum + batch.totalBytes, 0),
    ),
    currentGroupIndex: Math.min(index + 1, Math.max(0, batches.length - 1)),
    currentGroupName: batches[index + 1]?.groupName ?? operation.currentGroupName,
  };
}

/**
 * A cancellation the user asked for but which has not taken effect.
 *
 * The distinction the interface has to draw: "Cancelling" is a request in
 * flight, "Cancelled" is a finished operation. Showing the second while the
 * first is true tells the user files have stopped moving when they have not.
 */
export function requestCancellation(operation: ImportOperationProgress): ImportOperationProgress {
  if (!operation.canCancel) return operation;
  return { ...operation, cancellationRequested: true, phase: 'cancelling' };
}

/** Once a commit starts there is nothing safe to interrupt. */
export function enterCommit(operation: ImportOperationProgress): ImportOperationProgress {
  return { ...operation, phase: 'committing', canCancel: false };
}

export function enterPhase(
  operation: ImportOperationProgress,
  phase: ImportPhase,
): ImportOperationProgress {
  return { ...operation, phase, canCancel: phase === 'copying' || phase === 'preparing' };
}

export function startRollback(
  operation: ImportOperationProgress,
  total: number,
): ImportOperationProgress {
  return {
    ...operation,
    phase: 'rolling-back',
    canCancel: false,
    rollbackTotal: total,
    rollbackProcessed: 0,
  };
}

export function advanceRollback(
  operation: ImportOperationProgress,
  failure?: ImportOperationError,
): ImportOperationProgress {
  return {
    ...operation,
    rollbackProcessed: Math.min(operation.rollbackTotal, operation.rollbackProcessed + 1),
    errors: failure ? [...operation.errors, failure] : operation.errors,
  };
}

export function addError(
  operation: ImportOperationProgress,
  error: ImportOperationError,
): ImportOperationProgress {
  return { ...operation, errors: [...operation.errors, error] };
}

export function finish(
  operation: ImportOperationProgress,
  phase: 'completed' | 'failed',
): ImportOperationProgress {
  return {
    ...operation,
    phase,
    canCancel: false,
    // A completed operation shows a full bar even when the core's own totals
    // came in slightly under what was measured.
    processedEntries: phase === 'completed' ? operation.totalEntries : operation.processedEntries,
    processedBytes:
      phase === 'completed' && operation.totalBytes !== null
        ? operation.totalBytes
        : operation.processedBytes,
  };
}

/** 0–100, or null when the size is unknown and the bar is indeterminate. */
export function percentComplete(operation: ImportOperationProgress): number | null {
  if (operation.phase === 'completed') return 100;
  if (operation.totalBytes !== null && operation.totalBytes > 0) {
    return Math.min(100, (operation.processedBytes / operation.totalBytes) * 100);
  }
  // No bytes to weigh — an import of empty folders, say. Entries still say
  // something useful.
  if (operation.totalEntries > 0) {
    return Math.min(100, (operation.processedEntries / operation.totalEntries) * 100);
  }
  return null;
}

/** Whether the operation has stopped, one way or another. */
export function isFinished(operation: ImportOperationProgress): boolean {
  return operation.phase === 'completed' || operation.phase === 'failed';
}

/** Did a rollback fail to put something back? */
export function hasIncompleteRollback(operation: ImportOperationProgress): boolean {
  return operation.errors.some((error) => error.rollback === true);
}

/** What the dialog says the operation is doing. */
export function describePhase(operation: ImportOperationProgress): string {
  switch (operation.phase) {
    case 'validating':
      return 'Checking the plan';
    case 'preparing':
      return 'Preparing folders';
    case 'copying':
      return 'Copying files';
    case 'staging-replacements':
      return 'Moving replaced items aside';
    case 'committing':
      return 'Putting files in place';
    case 'finalising':
      return 'Finishing';
    case 'cancelling':
      return 'Cancelling — waiting for the current file to finish';
    case 'rolling-back':
      return `Undoing changes (${operation.rollbackProcessed} of ${operation.rollbackTotal})`;
    case 'completed':
      return operation.cancellationRequested ? 'Cancelled' : 'Completed';
    case 'failed':
      return hasIncompleteRollback(operation)
        ? 'Failed, and some changes could not be undone'
        : 'Failed';
  }
}

/**
 * How often the interface is allowed to redraw.
 *
 * The core reports per file. Ten thousand small files means ten thousand
 * renders, and the window stops answering. Coalescing to ~15 a second keeps it
 * responsive and is faster than anyone reads.
 */
export const PROGRESS_THROTTLE_MS = 66;

/** Should this event be drawn, given when the last one was? */
export function shouldRender(
  lastRenderedAt: number,
  now: number,
  event: { final?: boolean } = {},
): boolean {
  // The final state is always drawn: dropping it would leave the bar frozen
  // just short of the end.
  return event.final === true || now - lastRenderedAt >= PROGRESS_THROTTLE_MS;
}

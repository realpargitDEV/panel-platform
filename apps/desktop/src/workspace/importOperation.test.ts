import { describe, expect, it } from 'vitest';

import {
  addError,
  advanceRollback,
  applyBatchProgress,
  completeBatch,
  describePhase,
  enterCommit,
  enterPhase,
  finish,
  hasIncompleteRollback,
  isFinished,
  newOperation,
  percentComplete,
  PROGRESS_THROTTLE_MS,
  requestCancellation,
  shouldRender,
  startRollback,
  type ImportPhase,
  type OperationBatch,
} from './importOperation';

function batch(
  importId: string,
  groupName: string,
  entries: number,
  bytes: number,
): OperationBatch {
  return {
    importId,
    groupName,
    destination: '',
    sourcePaths: [`C:\\drop\\${groupName}`],
    unwrapPaths: [],
    destinationNames: [],
    replacePaths: [],
    totalEntries: entries,
    totalBytes: bytes,
  };
}

/** Two groups: 3 files / 300 bytes, then 2 files / 200 bytes. */
const batches = [batch('i1', 'bot', 3, 300), batch('i2', 'dashboard', 2, 200)];

describe('starting an operation', () => {
  it('totals every group up front', () => {
    const operation = newOperation('op-1', batches);
    expect(operation.totalGroups).toBe(2);
    expect(operation.totalEntries).toBe(5);
    expect(operation.totalBytes).toBe(500);
    expect(operation.processedEntries).toBe(0);
  });

  it('names the first group', () => {
    expect(newOperation('op-1', batches).currentGroupName).toBe('bot');
  });

  it('reports an unknown size as unknown rather than as zero', () => {
    // Nothing to import: the bar is indeterminate, not complete.
    expect(newOperation('op-1', []).totalBytes).toBeNull();
  });

  it('can cancel at the start', () => {
    expect(newOperation('op-1', batches).canCancel).toBe(true);
  });
});

describe('progress across groups', () => {
  it('adds the current batch to what earlier ones finished', () => {
    let operation = newOperation('op-1', batches);
    operation = applyBatchProgress(
      operation,
      batches,
      { entries: 3, bytes: 300 },
      {
        importId: 'i2',
        copiedFiles: 1,
        copiedBytes: 100,
        totalFiles: 2,
        totalBytes: 200,
        currentPath: 'client/index.ts',
      },
    );

    expect(operation.processedEntries).toBe(4);
    expect(operation.processedBytes).toBe(400);
  });

  it('does not reset when the next group starts from zero', () => {
    // The failure this exists to prevent: a bar that fills and empties per
    // group instead of once for the operation.
    let operation = newOperation('op-1', batches);
    operation = completeBatch(operation, batches, 'i1');
    expect(operation.processedBytes).toBe(300);

    operation = applyBatchProgress(
      operation,
      batches,
      { entries: 3, bytes: 300 },
      {
        importId: 'i2',
        copiedFiles: 0,
        copiedBytes: 0,
        totalFiles: 2,
        totalBytes: 200,
        currentPath: '',
      },
    );
    expect(operation.processedBytes).toBe(300);
  });

  it('never goes backwards even if a batch re-reports lower', () => {
    let operation = newOperation('op-1', batches);
    operation = applyBatchProgress(
      operation,
      batches,
      { entries: 0, bytes: 0 },
      {
        importId: 'i1',
        copiedFiles: 2,
        copiedBytes: 200,
        totalFiles: 3,
        totalBytes: 300,
        currentPath: 'a',
      },
    );
    operation = applyBatchProgress(
      operation,
      batches,
      { entries: 0, bytes: 0 },
      {
        importId: 'i1',
        copiedFiles: 1,
        copiedBytes: 50,
        totalFiles: 3,
        totalBytes: 300,
        currentPath: 'b',
      },
    );
    expect(operation.processedBytes).toBe(200);
  });

  it('tracks the current group and file', () => {
    const operation = applyBatchProgress(
      newOperation('op-1', batches),
      batches,
      { entries: 3, bytes: 300 },
      {
        importId: 'i2',
        copiedFiles: 1,
        copiedBytes: 10,
        totalFiles: 2,
        totalBytes: 200,
        currentPath: 'client/index.ts',
      },
    );
    expect(operation.currentGroupName).toBe('dashboard');
    expect(operation.currentGroupIndex).toBe(1);
    expect(operation.currentSourcePath).toBe('client/index.ts');
  });

  it('ignores an event from a batch it does not know', () => {
    const before = newOperation('op-1', batches);
    const after = applyBatchProgress(
      before,
      batches,
      { entries: 0, bytes: 0 },
      {
        importId: 'stale',
        copiedFiles: 99,
        copiedBytes: 9999,
        totalFiles: 99,
        totalBytes: 9999,
        currentPath: 'x',
      },
    );
    expect(after).toBe(before);
  });

  it('moves to the next group when one finishes', () => {
    const operation = completeBatch(newOperation('op-1', batches), batches, 'i1');
    expect(operation.currentGroupName).toBe('dashboard');
  });
});

describe('percentages', () => {
  it('weighs by bytes when they are known', () => {
    let operation = newOperation('op-1', batches);
    operation = completeBatch(operation, batches, 'i1');
    expect(percentComplete(operation)).toBe(60);
  });

  it('falls back to entries when there are no bytes to weigh', () => {
    const empty = [batch('i1', 'folders', 4, 0)];
    let operation = newOperation('op-1', empty);
    operation = applyBatchProgress(
      operation,
      empty,
      { entries: 0, bytes: 0 },
      {
        importId: 'i1',
        copiedFiles: 2,
        copiedBytes: 0,
        totalFiles: 0,
        totalBytes: 0,
        currentPath: '',
      },
    );
    expect(percentComplete(operation)).toBe(50);
  });

  it('is indeterminate when nothing can be weighed', () => {
    expect(percentComplete(newOperation('op-1', []))).toBeNull();
  });

  it('reads full once completed, whatever the core counted', () => {
    const operation = finish(newOperation('op-1', batches), 'completed');
    expect(percentComplete(operation)).toBe(100);
    expect(operation.processedEntries).toBe(5);
  });
});

describe('cancellation', () => {
  it('shows cancelling as soon as it is asked for', () => {
    const operation = requestCancellation(newOperation('op-1', batches));
    expect(operation.cancellationRequested).toBe(true);
    expect(operation.phase).toBe('cancelling');
  });

  it('keeps saying cancelling while later progress arrives', () => {
    // The copy in flight still reports; it must not look like normal progress.
    let operation = requestCancellation(newOperation('op-1', batches));
    operation = applyBatchProgress(
      operation,
      batches,
      { entries: 0, bytes: 0 },
      {
        importId: 'i1',
        copiedFiles: 1,
        copiedBytes: 100,
        totalFiles: 3,
        totalBytes: 300,
        currentPath: 'a',
      },
    );
    expect(operation.phase).toBe('cancelling');
  });

  it('cannot be cancelled once a commit has started', () => {
    const committing = enterCommit(newOperation('op-1', batches));
    expect(committing.canCancel).toBe(false);
    expect(requestCancellation(committing).cancellationRequested).toBe(false);
  });

  it('tells a cancelled operation apart from a completed one', () => {
    const cancelled = finish(requestCancellation(newOperation('op-1', batches)), 'completed');
    expect(describePhase(cancelled)).toBe('Cancelled');
    expect(describePhase(finish(newOperation('op-1', batches), 'completed'))).toBe('Completed');
  });
});

describe('rollback', () => {
  it('reports its own progress separately from the copy', () => {
    let operation = startRollback(newOperation('op-1', batches), 3);
    expect(operation.phase).toBe('rolling-back');
    expect(operation.canCancel).toBe(false);

    operation = advanceRollback(operation);
    expect(operation.rollbackProcessed).toBe(1);
    expect(describePhase(operation)).toContain('1 of 3');
  });

  it('records the exact path it could not put back', () => {
    let operation = startRollback(newOperation('op-1', batches), 1);
    operation = advanceRollback(operation, {
      path: 'src/app.ts',
      message: 'in use',
      rollback: true,
    });
    expect(hasIncompleteRollback(operation)).toBe(true);
    expect(operation.errors[0]?.path).toBe('src/app.ts');
  });

  it('says so when a failure left changes behind', () => {
    let operation = startRollback(newOperation('op-1', batches), 1);
    operation = advanceRollback(operation, { path: 'a', message: 'x', rollback: true });
    expect(describePhase(finish(operation, 'failed'))).toContain('could not be undone');
  });

  it('never counts past the total', () => {
    let operation = startRollback(newOperation('op-1', batches), 1);
    operation = advanceRollback(advanceRollback(operation));
    expect(operation.rollbackProcessed).toBe(1);
  });
});

describe('errors and finishing', () => {
  it('collects errors without stopping', () => {
    const operation = addError(newOperation('op-1', batches), {
      path: 'a.txt',
      message: 'refused',
    });
    expect(operation.errors).toHaveLength(1);
    expect(hasIncompleteRollback(operation)).toBe(false);
  });

  it('knows when it has stopped', () => {
    expect(isFinished(newOperation('op-1', batches))).toBe(false);
    expect(isFinished(finish(newOperation('op-1', batches), 'failed'))).toBe(true);
  });

  it('cannot be cancelled once finished', () => {
    expect(finish(newOperation('op-1', batches), 'failed').canCancel).toBe(false);
  });
});

describe('phases', () => {
  it('names each one for the dialog', () => {
    const operation = newOperation('op-1', batches);
    expect(describePhase(enterPhase(operation, 'validating'))).toBe('Checking the plan');
    expect(describePhase(enterPhase(operation, 'staging-replacements'))).toContain('aside');
    expect(describePhase(enterPhase(operation, 'copying'))).toBe('Copying files');
  });

  it('only allows cancelling during the interruptible phases', () => {
    const operation = newOperation('op-1', batches);
    expect(enterPhase(operation, 'copying').canCancel).toBe(true);
    expect(enterPhase(operation, 'finalising').canCancel).toBe(false);
    expect(enterPhase(operation, 'committing').canCancel).toBe(false);
  });

  /**
   * An organised import copies; it never moves the originals. A phase for
   * moving sources would be a state nothing can enter, which is how a dialog
   * ends up documenting behaviour the product does not have.
   */
  it('has no phase for moving sources, because no organised import moves any', () => {
    const phases: ImportPhase[] = [
      'validating',
      'preparing',
      'copying',
      'staging-replacements',
      'committing',
      'finalising',
      'cancelling',
      'rolling-back',
      'completed',
      'failed',
    ];
    const operation = newOperation('op-1', batches);
    for (const phase of phases) {
      expect(describePhase(enterPhase(operation, phase))).not.toMatch(/moving sources/i);
    }
    // @ts-expect-error `moving-sources` is not a phase any operation can enter.
    expect(() => enterPhase(operation, 'moving-sources')).toBeDefined();
  });
});

describe('throttling', () => {
  it('drops events that arrive too close together', () => {
    // Ten thousand tiny files would otherwise be ten thousand renders.
    expect(shouldRender(1000, 1000 + PROGRESS_THROTTLE_MS - 1)).toBe(false);
    expect(shouldRender(1000, 1000 + PROGRESS_THROTTLE_MS)).toBe(true);
  });

  it('always draws the final state, however soon it arrives', () => {
    expect(shouldRender(1000, 1001, { final: true })).toBe(true);
  });
});

/**
 * The progress dialog, as rendered.
 *
 * What these protect: one bar for the whole operation rather than one per
 * group, a cancel button that disables itself at the right moments, rollback
 * shown as its own thing, and a dialog that does not disappear on failure and
 * take the paths with it.
 */
import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import ImportProgressDialog from './ImportProgressDialog';
import {
  addError,
  advanceRollback,
  applyBatchProgress,
  completeBatch,
  enterCommit,
  finish,
  newOperation,
  requestCancellation,
  startRollback,
  type ImportOperationProgress,
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
    sourcePaths: [],
    unwrapPaths: [],
    destinationNames: [],
    replacePaths: [],
    totalEntries: entries,
    totalBytes: bytes,
  };
}

const batches = [batch('i1', 'bot', 3, 300), batch('i2', 'dashboard', 2, 200)];

function show(operation: ImportOperationProgress) {
  const onCancel = vi.fn();
  const onClose = vi.fn();
  const view = render(
    <ImportProgressDialog operation={operation} onCancel={onCancel} onClose={onClose} />,
  );
  return { onCancel, onClose, view };
}

function bar() {
  return screen.getByRole('progressbar');
}

describe('one bar for the whole operation', () => {
  it('counts every group in the totals', () => {
    show(newOperation('op-1', batches));
    expect(screen.getByText(/0 of 5 items/)).toBeInTheDocument();
  });

  it('names the group it is on', () => {
    show(newOperation('op-1', batches));
    expect(screen.getByText(/Group 1 of 2 · bot/)).toBeInTheDocument();
  });

  it('does not reset the percentage when the next group starts', () => {
    // The bug this replaced: three groups meant three bars filling from zero.
    let operation = completeBatch(newOperation('op-1', batches), batches, 'i1');
    const first = show(operation);
    expect(bar()).toHaveAttribute('aria-valuenow', '60');
    first.view.unmount();

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
    show(operation);
    expect(bar()).toHaveAttribute('aria-valuenow', '60');
    expect(screen.getByText(/Group 2 of 2 · dashboard/)).toBeInTheDocument();
  });

  it('shows the file it is on', () => {
    const operation = applyBatchProgress(
      newOperation('op-1', batches),
      batches,
      {
        entries: 0,
        bytes: 0,
      },
      {
        importId: 'i1',
        copiedFiles: 1,
        copiedBytes: 10,
        totalFiles: 3,
        totalBytes: 300,
        currentPath: 'src/index.ts',
      },
    );
    show(operation);
    expect(screen.getByText('src/index.ts')).toBeInTheDocument();
  });

  it('is indeterminate when the size cannot be known', () => {
    show(newOperation('op-1', []));
    expect(bar()).not.toHaveAttribute('aria-valuenow');
    expect(bar()).toHaveAttribute('aria-valuetext', 'Working, size unknown');
    expect(screen.getByText('size unknown')).toBeInTheDocument();
  });
});

describe('cancellation', () => {
  it('offers cancel while copying', () => {
    const { onCancel } = show(newOperation('op-1', batches));
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(onCancel).toHaveBeenCalledTimes(1);
  });

  it('says cancelling immediately, and stops offering it again', () => {
    show(requestCancellation(newOperation('op-1', batches)));
    const button = screen.getByRole('button', { name: 'Cancelling…' });
    expect(button).toBeDisabled();
    expect(screen.getByText(/waiting for the current file/i)).toBeInTheDocument();
  });

  it('disables cancel during a commit and says why', () => {
    show(enterCommit(newOperation('op-1', batches)));
    expect(screen.getByRole('button', { name: 'Cancel' })).toBeDisabled();
    expect(screen.getByText(/cannot be interrupted/i)).toBeInTheDocument();
  });

  it('tells cancelled apart from completed', () => {
    show(finish(requestCancellation(newOperation('op-1', batches)), 'completed'));
    expect(screen.getByText('Cancelled')).toBeInTheDocument();
    expect(screen.getByText(/Nothing was left half-imported/)).toBeInTheDocument();
  });
});

describe('rollback', () => {
  it('shows its own progress', () => {
    let operation = startRollback(newOperation('op-1', batches), 3);
    operation = advanceRollback(operation);
    show(operation);
    expect(screen.getByText(/Undoing 1 of 3 changes/)).toBeInTheDocument();
    expect(screen.getByText(/already in the project are not touched/)).toBeInTheDocument();
  });

  it('names the exact path it could not put back', () => {
    let operation = startRollback(newOperation('op-1', batches), 1);
    operation = advanceRollback(operation, {
      path: 'src/app.ts',
      message: 'still on disk at src/app.ts.replaced-ab12',
      rollback: true,
    });
    show(finish(operation, 'failed'));

    expect(screen.getByText(/Some changes could not be undone/)).toBeInTheDocument();
    expect(screen.getByText('src/app.ts')).toBeInTheDocument();
    expect(screen.getByText(/replaced-ab12/)).toBeInTheDocument();
  });
});

describe('finishing', () => {
  it('does not offer to close while it is still running', () => {
    show(newOperation('op-1', batches));
    expect(screen.queryByRole('button', { name: 'Close' })).toBeNull();
  });

  it('stays open on failure so the paths can be read', () => {
    const operation = addError(newOperation('op-1', batches), {
      path: 'src/a.ts',
      message: 'refused',
    });
    const { onClose } = show(finish(operation, 'failed'));

    expect(screen.getByText('Failed')).toBeInTheDocument();
    expect(screen.getByText('src/a.ts')).toBeInTheDocument();
    // It is the user who closes it, not a timer.
    expect(onClose).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Close' }));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it('summarises a success', () => {
    show(finish(newOperation('op-1', batches), 'completed'));
    expect(screen.getByText('Completed')).toBeInTheDocument();
    expect(screen.getByText('5 items imported.')).toBeInTheDocument();
    expect(bar()).toHaveAttribute('aria-valuenow', '100');
  });
});

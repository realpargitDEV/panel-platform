/**
 * The conflict dialog, as rendered.
 *
 * The contract these protect: Continue is unavailable while anything is
 * undecided, the decisions that reach the caller are the ones on screen, and
 * cancelling emits nothing at all.
 */
import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';

import ConflictDialog from './ConflictDialog';
import { allConflicts, analyse, type ItemKind, type PlannedItem } from './conflictResolution';

function item(source: string, destination: string, incoming: ItemKind = 'file'): PlannedItem {
  return { source, destination, incoming };
}

function conflictsFor(items: PlannedItem[], disk: Record<string, ItemKind>) {
  return allConflicts(analyse(items, new Map(Object.entries(disk)), 'copy'));
}

const fileConflicts = conflictsFor([item('a/x.ts', 'x.ts')], { 'x.ts': 'file' });

const mixedConflicts = conflictsFor([item('a/x.ts', 'x.ts'), item('a/src', 'src', 'directory')], {
  'x.ts': 'file',
  src: 'directory',
});

function renderDialog(conflicts = fileConflicts, existing = ['x.ts']) {
  const onConfirm = vi.fn();
  const onCancel = vi.fn();
  render(
    <ConflictDialog
      conflicts={conflicts}
      operation="copy"
      existing={existing}
      onConfirm={onConfirm}
      onCancel={onCancel}
    />,
  );
  return { onConfirm, onCancel };
}

function continueButton() {
  return screen.getByRole('button', { name: 'Continue' });
}

describe('refusing to continue', () => {
  it('disables Continue while a conflict is undecided', () => {
    renderDialog();
    expect(continueButton()).toBeDisabled();
  });

  it('enables it once every conflict has an answer', () => {
    renderDialog();
    fireEvent.click(screen.getByRole('button', { name: 'Skip x.ts' }));
    expect(continueButton()).toBeEnabled();
  });

  it('starts two folders on merge, so a folder-only batch is ready at once', () => {
    const folders = conflictsFor([item('a/src', 'src', 'directory')], { src: 'directory' });
    renderDialog(folders, ['src']);
    expect(continueButton()).toBeEnabled();
  });
});

describe('the choices offered', () => {
  it('offers merge for two folders and not for two files', () => {
    renderDialog(mixedConflicts, ['x.ts', 'src']);
    // One merge button, for the folder conflict only.
    expect(screen.getAllByRole('button', { name: /^Merge src$/ })).toHaveLength(1);
  });

  it('shows the resulting path when a rename is chosen', () => {
    renderDialog();
    fireEvent.click(screen.getByRole('button', { name: 'Keep both x.ts' }));
    expect(screen.getByText('x copy.ts')).toBeInTheDocument();
  });

  it('warns that a replace stages before deleting', () => {
    renderDialog();
    fireEvent.click(screen.getByRole('button', { name: 'Replace x.ts' }));
    expect(screen.getByText(/moved aside and deleted only once/i)).toBeInTheDocument();
  });
});

describe('applying to many at once', () => {
  it('settles every conflict from the apply-to-all row', () => {
    renderDialog(mixedConflicts, ['x.ts', 'src']);
    fireEvent.click(screen.getByRole('button', { name: 'Skip for all conflicts' }));
    expect(continueButton()).toBeEnabled();
  });

  it('resets every decision', () => {
    renderDialog();
    fireEvent.click(screen.getByRole('button', { name: 'Skip for all conflicts' }));
    expect(continueButton()).toBeEnabled();

    fireEvent.click(screen.getByRole('button', { name: 'Reset decisions' }));
    expect(continueButton()).toBeDisabled();
  });
});

describe('what reaches the caller', () => {
  it('emits the decisions that are on screen', () => {
    const { onConfirm } = renderDialog();
    fireEvent.click(screen.getByRole('button', { name: 'Replace x.ts' }));
    fireEvent.click(continueButton());

    expect(onConfirm).toHaveBeenCalledTimes(1);
    const decisions = onConfirm.mock.calls[0]?.[0] as Record<string, { resolution: string }>;
    expect(decisions['disk:a/x.ts']?.resolution).toBe('replace');
  });

  it('emits nothing when cancelled before any decision', () => {
    const { onCancel, onConfirm } = renderDialog();
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(onCancel).toHaveBeenCalledTimes(1);
    expect(onConfirm).not.toHaveBeenCalled();
  });

  it('asks before throwing away decisions that were made', () => {
    const { onCancel } = renderDialog();
    fireEvent.click(screen.getByRole('button', { name: 'Skip x.ts' }));
    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));

    // The first Cancel warns rather than closing.
    expect(onCancel).not.toHaveBeenCalled();
    expect(screen.getByText(/discards the decisions/i)).toBeInTheDocument();

    fireEvent.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(onCancel).toHaveBeenCalledTimes(1);
  });
});

describe('accessibility', () => {
  it('is a labelled modal dialog', () => {
    renderDialog();
    const dialog = screen.getByRole('dialog');
    expect(dialog).toHaveAttribute('aria-modal', 'true');
    expect(dialog).toHaveAccessibleName(/already exist/i);
  });

  it('marks the chosen resolution for assistive technology', () => {
    renderDialog();
    const skip = screen.getByRole('button', { name: 'Skip x.ts' });
    fireEvent.click(skip);
    expect(skip).toHaveAttribute('aria-pressed', 'true');
  });

  it('takes focus when it opens', () => {
    renderDialog();
    expect(screen.getByRole('dialog')).toHaveFocus();
  });

  it('closes on escape when nothing has been decided', () => {
    const { onCancel } = renderDialog();
    fireEvent.keyDown(screen.getByRole('dialog'), { key: 'Escape' });
    expect(onCancel).toHaveBeenCalledTimes(1);
  });
});

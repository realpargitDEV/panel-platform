/**
 * "Choose another destination", as rendered.
 *
 * The contract these protect: the picker never emits a destination the plan
 * cannot take, the final path is on screen before it is confirmed, cancelling
 * the picker keeps every decision already made, and the keyboard comes back to
 * the conflict that was being edited.
 *
 * The re-analysis itself is the caller's job — the dialog reports the request
 * and is remounted with the new conflicts — so what is asserted here is the
 * *request*, not the plan it produces. `relocation.test.ts` covers the plan.
 */
import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';

import ConflictDialog from './ConflictDialog';
import {
  allConflicts,
  analyse,
  type Decisions,
  type ItemKind,
  type PlannedItem,
} from './conflictResolution';
import type { PlanGrouping } from './relocation';

function item(source: string, destination: string, incoming: ItemKind = 'file'): PlannedItem {
  return { source, destination, incoming };
}

function conflictsFor(items: PlannedItem[], disk: Record<string, ItemKind>) {
  return allConflicts(analyse(items, new Map(Object.entries(disk)), 'import'));
}

const items = [item('/w/Dashboard/notes.md', 'notes.md'), item('/w/other.md', 'other.md')];
const conflicts = conflictsFor(items, { 'notes.md': 'file', 'other.md': 'file' });

const grouping: PlanGrouping = {
  groupOf: { '/w/Dashboard/notes.md': 'dash', '/w/other.md': 'loose' },
  rootOf: { dash: '', loose: '' },
};

function renderDialog(
  overrides: {
    directories?: string[];
    initialDecisions?: Decisions;
    notice?: string | null;
    grouping?: PlanGrouping;
  } = {},
) {
  const onRelocate = vi.fn();
  const onConfirm = vi.fn();
  const onCancel = vi.fn();
  render(
    <ConflictDialog
      conflicts={conflicts}
      operation="import"
      existing={['notes.md', 'other.md']}
      items={items}
      existingKinds={
        new Map<string, ItemKind>([
          ['notes.md', 'file'],
          ['other.md', 'file'],
          ['archive', 'directory'],
        ])
      }
      grouping={overrides.grouping ?? grouping}
      directories={overrides.directories ?? ['archive', 'docs']}
      initialDecisions={overrides.initialDecisions}
      notice={overrides.notice ?? null}
      onRelocate={onRelocate}
      onConfirm={onConfirm}
      onCancel={onCancel}
    />,
  );
  return { onRelocate, onConfirm, onCancel };
}

function openPickerFor(name = 'notes.md') {
  return screen.getByRole('button', { name: `Choose another destination for ${name}` });
}

function picker(name = 'notes.md') {
  return screen.getByRole('group', { name: `Choose another destination for ${name}` });
}

describe('offering the choice', () => {
  it('offers "Choose another destination" on every conflict', () => {
    renderDialog();
    expect(openPickerFor('notes.md')).toBeInTheDocument();
    expect(openPickerFor('other.md')).toBeInTheDocument();
  });

  it('does not offer it at all when the caller cannot handle a relocation', () => {
    render(
      <ConflictDialog
        conflicts={conflicts}
        operation="import"
        existing={['notes.md']}
        onConfirm={vi.fn()}
        onCancel={vi.fn()}
      />,
    );
    expect(
      screen.queryByRole('button', { name: /Choose another destination/ }),
    ).not.toBeInTheDocument();
  });

  it('opens the picker for the conflict it was pressed on', async () => {
    const user = userEvent.setup();
    renderDialog();
    await user.click(openPickerFor('notes.md'));
    expect(picker('notes.md')).toBeInTheDocument();
    expect(
      screen.queryByRole('group', { name: 'Choose another destination for other.md' }),
    ).not.toBeInTheDocument();
  });
});

describe('previewing before confirming', () => {
  it('shows the resulting final path as the destination is typed', async () => {
    const user = userEvent.setup();
    renderDialog();
    await user.click(openPickerFor());
    await user.type(screen.getByLabelText('Destination folder'), 'archive');
    expect(within(picker()).getByText('archive/notes.md')).toBeInTheDocument();
  });

  it('shows the group-relative path when the whole group moves', async () => {
    const user = userEvent.setup();
    renderDialog();
    await user.click(openPickerFor());
    await user.type(screen.getByLabelText('Destination folder'), 'Projects/New');
    await user.selectOptions(screen.getByLabelText('Apply to'), 'group');
    // The group's root is the project root, so the file keeps its own path
    // under the new destination rather than being rebuilt from its name.
    expect(within(picker()).getByText('Projects/New/notes.md')).toBeInTheDocument();
  });

  it('says when the folder does not exist yet and will be created', async () => {
    const user = userEvent.setup();
    renderDialog();
    await user.click(openPickerFor());
    await user.type(screen.getByLabelText('Destination folder'), 'BrandNew');
    expect(
      within(picker()).getByText(/does not exist yet and will be created/),
    ).toBeInTheDocument();
  });
});

describe('refusing a destination the plan cannot take', () => {
  async function typeDestination(value: string) {
    const user = userEvent.setup();
    renderDialog();
    await user.click(openPickerFor());
    await user.type(screen.getByLabelText('Destination folder'), value);
    return user;
  }

  it('refuses an empty destination', async () => {
    const user = userEvent.setup();
    const { onRelocate } = renderDialog();
    await user.click(openPickerFor());
    expect(within(picker()).getByRole('alert')).toHaveTextContent(/Enter a destination/);
    expect(screen.getByRole('button', { name: /Use this destination for/ })).toBeDisabled();
    expect(onRelocate).not.toHaveBeenCalled();
  });

  it('refuses a path that steps outside the project', async () => {
    await typeDestination('../outside');
    expect(within(picker()).getByRole('alert')).toHaveTextContent(/\.\./);
  });

  it('refuses an absolute path', async () => {
    await typeDestination('C:/Windows');
    expect(within(picker()).getByRole('alert')).toHaveTextContent(/inside the project/);
  });

  it('refuses characters a folder name cannot hold', async () => {
    await typeDestination('bad?name');
    expect(within(picker()).getByRole('alert')).toHaveTextContent(/cannot have/);
  });

  it('refuses a destination that is a file', async () => {
    await typeDestination('other.md');
    expect(within(picker()).getByRole('alert')).toHaveTextContent(/is a file/);
  });

  it('refuses a destination another incoming entry already lands in', async () => {
    const user = userEvent.setup();
    render(
      <ConflictDialog
        conflicts={conflicts}
        operation="import"
        existing={['notes.md']}
        items={[item('/w/Dashboard/notes.md', 'notes.md'), item('/w/x/notes.md', 'docs/notes.md')]}
        grouping={grouping}
        directories={['docs']}
        onRelocate={vi.fn()}
        onConfirm={vi.fn()}
        onCancel={vi.fn()}
      />,
    );
    await user.click(openPickerFor());
    await user.type(screen.getByLabelText('Destination folder'), 'docs');
    expect(within(picker()).getByRole('alert')).toHaveTextContent(/already lands at/);
  });

  it('refuses moving a folder into itself', async () => {
    const user = userEvent.setup();
    const folder = conflictsFor([item('/w/src', 'src', 'directory')], { src: 'directory' });
    render(
      <ConflictDialog
        conflicts={folder}
        operation="import"
        existing={['src']}
        items={[item('/w/src', 'src', 'directory')]}
        directories={['src']}
        onRelocate={vi.fn()}
        onConfirm={vi.fn()}
        onCancel={vi.fn()}
      />,
    );
    await user.click(screen.getByRole('button', { name: 'Choose another destination for src' }));
    await user.type(screen.getByLabelText('Destination folder'), 'src/nested');
    expect(screen.getByRole('alert')).toHaveTextContent(/into itself|its own folders/);
  });
});

describe('emitting the request', () => {
  it('sends the destination and the scope the user chose', async () => {
    const user = userEvent.setup();
    const { onRelocate } = renderDialog();
    await user.click(openPickerFor());
    await user.type(screen.getByLabelText('Destination folder'), 'archive');
    await user.click(screen.getByRole('button', { name: /Use this destination for notes.md/ }));

    expect(onRelocate).toHaveBeenCalledTimes(1);
    expect(onRelocate.mock.calls[0]?.[0]).toEqual({
      conflictId: conflicts[0]?.id,
      destination: 'archive',
      scope: 'one',
    });
  });

  it('sends the group scope when the whole group is moved', async () => {
    const user = userEvent.setup();
    const { onRelocate } = renderDialog();
    await user.click(openPickerFor());
    await user.type(screen.getByLabelText('Destination folder'), 'Projects');
    await user.selectOptions(screen.getByLabelText('Apply to'), 'group');
    await user.click(screen.getByRole('button', { name: /Use this destination for notes.md/ }));
    expect(onRelocate.mock.calls[0]?.[0]?.scope).toBe('group');
  });

  it('offers the project root as a destination of its own', async () => {
    const user = userEvent.setup();
    const { onRelocate } = renderDialog();
    await user.click(openPickerFor());
    await user.click(screen.getByRole('button', { name: /Use the project root/ }));
    expect(onRelocate.mock.calls[0]?.[0]?.destination).toBe('');
  });

  it('can take a folder that already exists from the list', async () => {
    const user = userEvent.setup();
    const { onRelocate } = renderDialog();
    await user.click(openPickerFor());
    await user.selectOptions(screen.getByLabelText(/folder that already exists/), 'docs');
    await user.click(screen.getByRole('button', { name: /Use this destination for notes.md/ }));
    expect(onRelocate.mock.calls[0]?.[0]?.destination).toBe('docs');
  });

  it('carries the decisions already made along with the request', async () => {
    const user = userEvent.setup();
    const { onRelocate } = renderDialog();
    await user.click(screen.getByRole('button', { name: 'Skip other.md' }));
    await user.click(openPickerFor());
    await user.type(screen.getByLabelText('Destination folder'), 'archive');
    await user.click(screen.getByRole('button', { name: /Use this destination for notes.md/ }));

    const carried = onRelocate.mock.calls[0]?.[1] as Decisions;
    expect(carried[conflicts[1]!.id]).toEqual({ resolution: 'skip' });
  });
});

describe('keeping the rest of the review intact', () => {
  it('keeps decisions when the picker is cancelled', async () => {
    const user = userEvent.setup();
    const { onRelocate, onCancel } = renderDialog();
    await user.click(screen.getByRole('button', { name: 'Skip other.md' }));
    await user.click(openPickerFor());
    await user.click(screen.getByRole('button', { name: /Cancel choosing a destination/ }));

    expect(onRelocate).not.toHaveBeenCalled();
    // The dialog itself did not close, and the decision is still shown.
    expect(onCancel).not.toHaveBeenCalled();
    expect(screen.getByRole('button', { name: 'Skip other.md' })).toHaveAttribute(
      'aria-pressed',
      'true',
    );
  });

  it('closes only the picker on Escape, not the whole review', async () => {
    const user = userEvent.setup();
    const { onCancel } = renderDialog();
    await user.click(openPickerFor());
    await user.keyboard('{Escape}');
    expect(
      screen.queryByRole('group', { name: /Choose another destination for notes.md/ }),
    ).not.toBeInTheDocument();
    expect(onCancel).not.toHaveBeenCalled();
  });

  it('returns focus to the conflict that was being edited', async () => {
    const user = userEvent.setup();
    renderDialog();
    const trigger = openPickerFor();
    await user.click(trigger);
    await user.click(screen.getByRole('button', { name: /Cancel choosing a destination/ }));
    await vi.waitFor(() => expect(openPickerFor()).toHaveFocus());
  });
});

describe('after a re-analysis', () => {
  it('starts from the decisions the caller preserved', () => {
    renderDialog({ initialDecisions: { [conflicts[1]!.id]: { resolution: 'skip' } } });
    expect(screen.getByRole('button', { name: 'Skip other.md' })).toHaveAttribute(
      'aria-pressed',
      'true',
    );
  });

  it('still refuses to continue while a new conflict is unresolved', () => {
    renderDialog({ initialDecisions: { [conflicts[1]!.id]: { resolution: 'skip' } } });
    // `notes.md` was invalidated by the move and came back unresolved.
    expect(screen.getByRole('button', { name: 'Continue' })).toBeDisabled();
  });

  it('announces what the destination change did', () => {
    renderDialog({
      notice: 'That created 1 new conflict, which must be decided before continuing.',
    });
    expect(screen.getByRole('status')).toHaveTextContent(/1 new conflict/);
  });
});

describe('accessibility', () => {
  it('names every control in the picker', async () => {
    const user = userEvent.setup();
    renderDialog();
    await user.click(openPickerFor());
    expect(screen.getByLabelText('Destination folder')).toBeInTheDocument();
    expect(screen.getByLabelText('Apply to')).toBeInTheDocument();
    expect(screen.getByLabelText(/folder that already exists/)).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Use the project root/ })).toBeInTheDocument();
    expect(screen.getByRole('button', { name: /Use this destination for/ })).toBeInTheDocument();
    expect(
      screen.getByRole('button', { name: /Cancel choosing a destination/ }),
    ).toBeInTheDocument();
  });

  it('says whether the picker is open', async () => {
    const user = userEvent.setup();
    renderDialog();
    expect(openPickerFor()).toHaveAttribute('aria-expanded', 'false');
    await user.click(openPickerFor());
    expect(openPickerFor()).toHaveAttribute('aria-expanded', 'true');
  });

  it('keeps focus inside the dialog while the picker is open', async () => {
    const user = userEvent.setup();
    renderDialog();
    await user.click(openPickerFor());
    const dialog = screen.getByRole('dialog');
    for (let press = 0; press < 12; press += 1) {
      await user.tab();
      expect(dialog).toContainElement(document.activeElement as HTMLElement);
    }
  });
});

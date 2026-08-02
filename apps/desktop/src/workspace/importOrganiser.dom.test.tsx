/**
 * The Import Organiser, as rendered.
 *
 * The 38 tests in `importGroups.test.ts` prove the plan arithmetic. These prove
 * that the screen is wired to it: that what is drawn reflects the groups, that
 * every control emits the change it claims to, and that pressing Import hands
 * over the plan the user is actually looking at.
 *
 * Everything is asserted through visible output and emitted requests. No test
 * reaches into component state, because state is not the contract — a group
 * that renders as excluded but still imports is the bug worth catching, and
 * only the emitted plan can catch it.
 */
import { fireEvent, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { useState } from 'react';
import { describe, expect, it, vi } from 'vitest';

import ImportOrganiser from './ImportOrganiser';
import { groupsFrom, planFrom, type ImportGroup } from './importGroups';
import type { ImportCandidate } from '../api';

// ------------------------------------------------------------------ fixtures

function candidate(overrides: Partial<ImportCandidate> & { path: string }): ImportCandidate {
  return {
    name: overrides.path.split('/').pop() ?? overrides.path,
    isDirectory: true,
    isProject: false,
    score: 0,
    signals: [],
    children: [],
    childCount: 0,
    ecosystem: null,
    isMonorepo: false,
    nested: [],
    ...overrides,
  };
}

function nested(relative: string, belongsToWorkspace = true) {
  return {
    path: `/w/Monorepo/${relative}`,
    relative,
    name: relative.split('/').pop() ?? relative,
    ecosystem: 'node',
    score: 60,
    belongsToWorkspace,
  };
}

const dashboard = candidate({
  path: '/w/Dashboard',
  name: 'Dashboard',
  isProject: true,
  score: 82,
  signals: ['package.json', 'tsconfig.json'],
  ecosystem: 'node',
});

const workspace = candidate({
  path: '/w/Monorepo',
  name: 'Monorepo',
  isProject: true,
  score: 91,
  signals: ['pnpm-workspace.yaml'],
  ecosystem: 'node',
  isMonorepo: true,
  nested: [nested('packages/api'), nested('packages/web')],
});

const plainFolder = candidate({ path: '/w/Assets', name: 'Assets' });

const ambiguous = candidate({
  path: '/w/Maybe',
  name: 'Maybe',
  isProject: false,
  score: 18,
  signals: ['Makefile'],
});

const looseFile = candidate({
  path: '/w/notes.md',
  name: 'notes.md',
  isDirectory: false,
  isProject: false,
});

const candidates: ImportCandidate[] = [dashboard, workspace, plainFolder, ambiguous, looseFile];

function renderOrganiser(
  initial: ImportGroup[] = groupsFrom(candidates, ''),
  options: { existingPaths?: string[]; destination?: string } = {},
) {
  const onImport = vi.fn();
  const onCancel = vi.fn();
  const state = { groups: initial };

  function Harness() {
    // The organiser is controlled: the production flow owns the groups and
    // feeds them back. Mirroring that here is what makes an emitted `onChange`
    // show up on screen, which is the only way to test it honestly.
    const [groups, setGroups] = useState(initial);
    state.groups = groups;
    return (
      <ImportOrganiser
        groups={groups}
        candidates={candidates}
        destination={options.destination ?? ''}
        existingPaths={options.existingPaths ?? []}
        onChange={setGroups}
        onImport={() => onImport(planFrom(groups), groups)}
        onCancel={onCancel}
      />
    );
  }

  render(<Harness />);
  return { onImport, onCancel, state };
}

function groupSection(name: string): HTMLElement {
  const heading = screen.getByRole('button', { name });
  const section = heading.closest('section');
  if (!section) throw new Error(`no section for ${name}`);
  return section;
}

function entryRow(path: string): HTMLElement {
  const row = document.querySelector(`[data-path="${path}"]`);
  if (!(row instanceof HTMLElement)) throw new Error(`no row for ${path}`);
  return row;
}

// -------------------------------------------------------- rendering & kinds

describe('rendering and classification', () => {
  it('renders a detected project group', () => {
    renderOrganiser();
    expect(screen.getByRole('button', { name: 'Dashboard' })).toBeInTheDocument();
    expect(
      within(groupSection('Dashboard')).getByText(/Project/, { selector: 'span' }),
    ).toBeInTheDocument();
  });

  it('renders a normal folder as a folder', () => {
    renderOrganiser();
    expect(
      within(groupSection('Assets')).getByText(/Folder/, { selector: 'span' }),
    ).toBeInTheDocument();
  });

  it('renders standalone files in their own group', () => {
    renderOrganiser();
    const section = groupSection('Standalone files');
    expect(within(section).getByText('notes.md')).toBeInTheDocument();
  });

  it('renders a group the detection was unsure about', () => {
    renderOrganiser();
    expect(screen.getByRole('button', { name: 'Maybe' })).toBeInTheDocument();
  });

  it('shows the suggested project name', () => {
    renderOrganiser();
    expect(screen.getByRole('button', { name: 'Dashboard' })).toHaveTextContent('Dashboard');
  });

  it('shows the confidence score in the explanation', () => {
    renderOrganiser();
    expect(within(groupSection('Dashboard')).getByText(/score 82/)).toBeInTheDocument();
  });

  it('makes the detection markers and the reasoning readable', () => {
    renderOrganiser();
    const section = groupSection('Dashboard');
    expect(within(section).getByText(/package\.json/)).toBeInTheDocument();
    expect(within(section).getByText(/Detected as a node project/)).toBeInTheDocument();
  });

  it('explains why an ambiguous folder was not called a project', () => {
    renderOrganiser();
    expect(
      within(groupSection('Maybe')).getByText(/not enough to call it a project/),
    ).toBeInTheDocument();
  });

  it('marks a workspace and keeps its members inside it', () => {
    renderOrganiser();
    const section = groupSection('Monorepo');
    expect(within(section).getByText(/workspace/, { selector: 'span' })).toBeInTheDocument();
    expect(within(section).getByText(/packages\/api, packages\/web/)).toBeInTheDocument();
  });

  it('does not split a monorepo child out into its own group', () => {
    renderOrganiser();
    expect(screen.queryByRole('button', { name: 'packages/api' })).not.toBeInTheDocument();
  });

  it('summarises what will be imported', () => {
    renderOrganiser();
    expect(screen.getByText(/2 projects, 2 folders and 1 file/)).toBeInTheDocument();
  });
});

// -------------------------------------------------------------- group controls

describe('group controls', () => {
  it('excludes a group and leaves it out of the plan', async () => {
    const user = userEvent.setup();
    const { onImport } = renderOrganiser();
    await user.click(screen.getByLabelText('Include Assets'));
    await user.click(screen.getByRole('button', { name: 'Import' }));

    const [plan] = onImport.mock.calls[0] as [ReturnType<typeof planFrom>];
    expect(plan.batches.flatMap((batch) => batch.sourcePaths)).not.toContain('/w/Assets');
    expect(plan.excluded).toBeGreaterThan(0);
  });

  it('includes a group again', async () => {
    const user = userEvent.setup();
    const { onImport } = renderOrganiser();
    await user.click(screen.getByLabelText('Include Assets'));
    await user.click(screen.getByLabelText('Include Assets'));
    await user.click(screen.getByRole('button', { name: 'Import' }));

    const [plan] = onImport.mock.calls[0] as [ReturnType<typeof planFrom>];
    expect(plan.batches.flatMap((batch) => batch.sourcePaths)).toContain('/w/Assets');
  });

  it('renames a group and lands it under the new name', async () => {
    const user = userEvent.setup();
    const { onImport } = renderOrganiser();
    await user.dblClick(screen.getByRole('button', { name: 'Assets' }));
    const field = screen.getByLabelText('Group name');
    await user.clear(field);
    await user.type(field, 'Artwork{Enter}');

    expect(screen.getByRole('button', { name: 'Artwork' })).toBeInTheDocument();
    await user.click(screen.getByRole('button', { name: 'Import' }));
    const [plan] = onImport.mock.calls[0] as [ReturnType<typeof planFrom>];
    expect(plan.destinations.map((entry) => entry.path)).toContain('Artwork');
  });

  it('turns a project into a normal folder, which keeps its wrapper', async () => {
    const user = userEvent.setup();
    const { onImport } = renderOrganiser();
    const kind = within(groupSection('Dashboard')).getByLabelText('Kind');
    await user.selectOptions(kind, 'folder');

    await user.click(screen.getByRole('button', { name: 'Import' }));
    const [plan] = onImport.mock.calls[0] as [ReturnType<typeof planFrom>];
    expect(plan.batches.flatMap((batch) => batch.unwrapPaths)).not.toContain('/w/Dashboard');
  });

  it('turns a normal folder into a project, which unwraps it', async () => {
    const user = userEvent.setup();
    const { onImport } = renderOrganiser();
    // Everything else stops unwrapping into the root first, so this one can.
    await user.selectOptions(within(groupSection('Dashboard')).getByLabelText('Kind'), 'folder');
    await user.selectOptions(within(groupSection('Monorepo')).getByLabelText('Kind'), 'folder');
    await user.selectOptions(within(groupSection('Assets')).getByLabelText('Kind'), 'project');

    await user.click(screen.getByRole('button', { name: 'Import' }));
    const [plan] = onImport.mock.calls[0] as [ReturnType<typeof planFrom>];
    expect(plan.batches.flatMap((batch) => batch.unwrapPaths)).toContain('/w/Assets');
  });

  it('switches a group between keeping its folder and unwrapping it', async () => {
    const user = userEvent.setup();
    const { onImport } = renderOrganiser();
    const layout = within(groupSection('Assets')).getByLabelText('Layout');
    expect(layout).toHaveValue('keep');
    await user.selectOptions(layout, 'unwrap');

    await user.click(screen.getByRole('button', { name: 'Import' }));
    const [plan] = onImport.mock.calls[0] as [ReturnType<typeof planFrom>];
    expect(plan.batches.flatMap((batch) => batch.unwrapPaths)).toContain('/w/Assets');
  });

  it('takes a custom destination for one group', async () => {
    const user = userEvent.setup();
    const { onImport } = renderOrganiser();
    const into = within(groupSection('Assets')).getByLabelText('Into');
    await user.type(into, 'media');

    await user.click(screen.getByRole('button', { name: 'Import' }));
    const [plan] = onImport.mock.calls[0] as [ReturnType<typeof planFrom>];
    expect(plan.batches.map((batch) => batch.destination)).toContain('media');
  });

  it('offers Reset only once a group has been changed, and puts it back', async () => {
    const user = userEvent.setup();
    renderOrganiser();
    const section = groupSection('Assets');
    expect(within(section).queryByRole('button', { name: 'Reset' })).not.toBeInTheDocument();

    await user.selectOptions(within(section).getByLabelText('Kind'), 'project');
    await user.click(within(groupSection('Assets')).getByRole('button', { name: 'Reset' }));

    expect(within(groupSection('Assets')).getByLabelText('Kind')).toHaveValue('folder');
  });

  it('creates a custom group', async () => {
    const user = userEvent.setup();
    renderOrganiser();
    await user.click(screen.getByRole('button', { name: 'New group' }));
    expect(screen.getByRole('button', { name: /^Group \d/ })).toBeInTheDocument();
  });

  it('merges the included groups into one', async () => {
    const user = userEvent.setup();
    const { onImport } = renderOrganiser();
    await user.click(screen.getByRole('button', { name: 'Merge included' }));

    await user.click(screen.getByRole('button', { name: 'Import' }));
    const [, groups] = onImport.mock.calls[0] as [unknown, ImportGroup[]];
    const included = groups.filter((group) => group.include && group.entries.length > 0);
    expect(included).toHaveLength(1);
    // Nothing is lost in the merge.
    expect(included[0]?.entries).toHaveLength(5);
  });

  it('refuses a merge there is nothing to merge with', async () => {
    const user = userEvent.setup();
    const single = groupsFrom([plainFolder], '');
    renderOrganiser(single);
    expect(screen.getByRole('button', { name: 'Merge included' })).toBeDisabled();
    await user.click(screen.getByRole('button', { name: 'Merge included' }));
    expect(screen.getByRole('button', { name: 'Assets' })).toBeInTheDocument();
  });
});

// -------------------------------------------------- selection and movement

describe('entry selection and movement', () => {
  it('selects one entry on click', async () => {
    const user = userEvent.setup();
    renderOrganiser();
    await user.click(entryRow('/w/Assets'));
    expect(entryRow('/w/Assets').className).toMatch(/bg-vs-active/);
  });

  it('adds to the selection with ctrl-click', () => {
    renderOrganiser();
    fireEvent.mouseDown(entryRow('/w/Assets'));
    fireEvent.mouseDown(entryRow('/w/Maybe'), { ctrlKey: true });
    expect(entryRow('/w/Assets').className).toMatch(/bg-vs-active/);
    expect(entryRow('/w/Maybe').className).toMatch(/bg-vs-active/);
  });

  it('selects a range with shift-click', () => {
    renderOrganiser();
    fireEvent.mouseDown(entryRow('/w/Dashboard'));
    fireEvent.mouseDown(entryRow('/w/Assets'), { shiftKey: true });
    expect(entryRow('/w/Monorepo').className).toMatch(/bg-vs-active/);
  });

  it('moves a dragged entry into another group, and the plan follows', async () => {
    const user = userEvent.setup();
    const { onImport } = renderOrganiser();

    const data = new Map<string, string>();
    const dataTransfer = {
      setData: (type: string, value: string) => data.set(type, value),
      getData: (type: string) => data.get(type) ?? '',
      get types() {
        return [...data.keys()];
      },
      effectAllowed: 'move',
    };

    fireEvent.dragStart(entryRow('/w/notes.md'), { dataTransfer });
    fireEvent.drop(groupSection('Dashboard'), { dataTransfer });

    // The file is now drawn inside the Dashboard group.
    expect(within(groupSection('Dashboard')).getByText('notes.md')).toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Import' }));
    const [, groups] = onImport.mock.calls[0] as [unknown, ImportGroup[]];
    const dash = groups.find((group) => group.id === '/w/Dashboard');
    expect(dash?.entries.map((entry) => entry.path)).toContain('/w/notes.md');
  });

  it('moves every selected entry together', () => {
    renderOrganiser();
    fireEvent.mouseDown(entryRow('/w/Assets'));
    fireEvent.mouseDown(entryRow('/w/Maybe'), { ctrlKey: true });

    const data = new Map<string, string>();
    const dataTransfer = {
      setData: (type: string, value: string) => data.set(type, value),
      getData: (type: string) => data.get(type) ?? '',
      get types() {
        return [...data.keys()];
      },
      effectAllowed: 'move',
    };

    fireEvent.dragStart(entryRow('/w/Assets'), { dataTransfer });
    fireEvent.drop(groupSection('Dashboard'), { dataTransfer });

    const section = groupSection('Dashboard');
    expect(within(section).getByText('Assets/')).toBeInTheDocument();
    expect(within(section).getByText('Maybe/')).toBeInTheDocument();
  });

  it('regrouping changes the plan and never the source path', async () => {
    const user = userEvent.setup();
    const { onImport } = renderOrganiser();

    const data = new Map<string, string>();
    const dataTransfer = {
      setData: (type: string, value: string) => data.set(type, value),
      getData: (type: string) => data.get(type) ?? '',
      get types() {
        return [...data.keys()];
      },
      effectAllowed: 'move',
    };
    fireEvent.dragStart(entryRow('/w/notes.md'), { dataTransfer });
    fireEvent.drop(groupSection('Dashboard'), { dataTransfer });

    await user.click(screen.getByRole('button', { name: 'Import' }));
    const [, groups] = onImport.mock.calls[0] as [unknown, ImportGroup[]];
    const moved = groups
      .flatMap((group) => group.entries)
      .find((entry) => entry.name === 'notes.md');
    // The entry is identified by where it is on this machine, and that is what
    // the import reads from. Regrouping must not have rewritten it.
    expect(moved?.path).toBe('/w/notes.md');
  });

  it('removes an entry from the import entirely', async () => {
    const user = userEvent.setup();
    const { onImport } = renderOrganiser();
    await user.click(screen.getByRole('button', { name: 'Remove Assets from the import' }));

    await user.click(screen.getByRole('button', { name: 'Import' }));
    const [plan] = onImport.mock.calls[0] as [ReturnType<typeof planFrom>];
    expect(plan.batches.flatMap((batch) => batch.sourcePaths)).not.toContain('/w/Assets');
  });

  it('moves a standalone file into a detected project', () => {
    renderOrganiser();
    const data = new Map<string, string>();
    const dataTransfer = {
      setData: (type: string, value: string) => data.set(type, value),
      getData: (type: string) => data.get(type) ?? '',
      get types() {
        return [...data.keys()];
      },
      effectAllowed: 'move',
    };
    fireEvent.dragStart(entryRow('/w/notes.md'), { dataTransfer });
    fireEvent.drop(groupSection('Monorepo'), { dataTransfer });
    expect(within(groupSection('Monorepo')).getByText('notes.md')).toBeInTheDocument();
  });
});

// ------------------------------------------------------- layout normalisation

describe('layout normalisation', () => {
  it('lets only one group unwrap into the same destination', async () => {
    const user = userEvent.setup();
    renderOrganiser();
    // Two projects both start wanting the root; the rules keep their folders.
    const dash = within(groupSection('Dashboard')).getByLabelText('Layout');
    const mono = within(groupSection('Monorepo')).getByLabelText('Layout');
    expect(dash).toHaveValue('keep');
    expect(mono).toHaveValue('keep');

    // With one of them gone, the other may unwrap.
    await user.click(screen.getByLabelText('Include Monorepo'));
    await user.selectOptions(within(groupSection('Dashboard')).getByLabelText('Layout'), 'unwrap');
    expect(within(groupSection('Dashboard')).getByLabelText('Layout')).toHaveValue('unwrap');
  });

  it('blocks a second group from unwrapping into a destination already claimed', async () => {
    const user = userEvent.setup();
    renderOrganiser();
    await user.click(screen.getByLabelText('Include Monorepo'));
    await user.selectOptions(within(groupSection('Dashboard')).getByLabelText('Layout'), 'unwrap');
    await user.click(screen.getByLabelText('Include Monorepo'));
    await user.selectOptions(within(groupSection('Monorepo')).getByLabelText('Layout'), 'unwrap');

    // Both wanted the root, so neither gets it.
    expect(within(groupSection('Dashboard')).getByLabelText('Layout')).toHaveValue('keep');
    expect(within(groupSection('Monorepo')).getByLabelText('Layout')).toHaveValue('keep');
  });

  it('lets two wrapped groups share a parent when their paths do not collide', async () => {
    const user = userEvent.setup();
    const { onImport } = renderOrganiser();
    await user.type(within(groupSection('Assets')).getByLabelText('Into'), 'shared');
    await user.type(within(groupSection('Maybe')).getByLabelText('Into'), 'shared');

    await user.click(screen.getByRole('button', { name: 'Import' }));
    const [plan] = onImport.mock.calls[0] as [ReturnType<typeof planFrom>];
    const paths = plan.destinations.map((entry) => entry.path);
    expect(paths).toContain('shared/Assets');
    expect(paths).toContain('shared/Maybe');
  });

  it('recalculates the landing path when a wrapped group is renamed', async () => {
    const user = userEvent.setup();
    const { onImport } = renderOrganiser();
    await user.type(within(groupSection('Assets')).getByLabelText('Into'), 'media');
    await user.dblClick(screen.getByRole('button', { name: 'Assets' }));
    const field = screen.getByLabelText('Group name');
    await user.clear(field);
    await user.type(field, 'Artwork{Enter}');

    await user.click(screen.getByRole('button', { name: 'Import' }));
    const [plan] = onImport.mock.calls[0] as [ReturnType<typeof planFrom>];
    expect(plan.destinations.map((entry) => entry.path)).toContain('media/Artwork');
  });

  it('warns when a custom destination collides with what is already there', async () => {
    const user = userEvent.setup();
    renderOrganiser(groupsFrom(candidates, ''), { existingPaths: ['media/Assets'] });
    await user.type(within(groupSection('Assets')).getByLabelText('Into'), 'media');
    expect(screen.getByText(/already exist/)).toBeInTheDocument();
    expect(screen.getByText(/media\/Assets/)).toBeInTheDocument();
  });

  it('reset restores the automatic layout and destination', async () => {
    const user = userEvent.setup();
    renderOrganiser();
    const into = within(groupSection('Assets')).getByLabelText('Into');
    await user.type(into, 'media');
    await user.click(within(groupSection('Assets')).getByRole('button', { name: 'Reset' }));
    expect(within(groupSection('Assets')).getByLabelText('Into')).toHaveValue('');
  });
});

// ------------------------------------------------------------- the import call

describe('starting the import', () => {
  it('sends the whole organiser plan', async () => {
    const user = userEvent.setup();
    const { onImport } = renderOrganiser();
    await user.click(screen.getByRole('button', { name: 'Import' }));

    const [plan] = onImport.mock.calls[0] as [ReturnType<typeof planFrom>];
    const sources = plan.batches.flatMap((batch) => batch.sourcePaths);
    expect(sources).toEqual(
      expect.arrayContaining(['/w/Dashboard', '/w/Monorepo', '/w/Assets', '/w/notes.md']),
    );
  });

  it('omits excluded groups and removed entries', async () => {
    const user = userEvent.setup();
    const { onImport } = renderOrganiser();
    await user.click(screen.getByLabelText('Include Assets'));
    await user.click(screen.getByRole('button', { name: 'Remove notes.md from the import' }));
    await user.click(screen.getByRole('button', { name: 'Import' }));

    const [plan] = onImport.mock.calls[0] as [ReturnType<typeof planFrom>];
    const sources = plan.batches.flatMap((batch) => batch.sourcePaths);
    expect(sources).not.toContain('/w/Assets');
    expect(sources).not.toContain('/w/notes.md');
  });

  it('sends wrapper and unwrap decisions through to the plan', async () => {
    const user = userEvent.setup();
    const { onImport } = renderOrganiser();
    await user.click(screen.getByLabelText('Include Monorepo'));
    await user.selectOptions(within(groupSection('Dashboard')).getByLabelText('Layout'), 'unwrap');
    await user.click(screen.getByRole('button', { name: 'Import' }));

    const [plan] = onImport.mock.calls[0] as [ReturnType<typeof planFrom>];
    expect(plan.batches.flatMap((batch) => batch.unwrapPaths)).toContain('/w/Dashboard');
    expect(plan.batches.flatMap((batch) => batch.unwrapPaths)).not.toContain('/w/Assets');
  });

  it('groups the batches by destination, one call per directory', async () => {
    const user = userEvent.setup();
    const { onImport } = renderOrganiser();
    await user.type(within(groupSection('Assets')).getByLabelText('Into'), 'media');
    await user.click(screen.getByRole('button', { name: 'Import' }));

    const [plan] = onImport.mock.calls[0] as [ReturnType<typeof planFrom>];
    expect(new Set(plan.batches.map((batch) => batch.destination))).toEqual(new Set(['', 'media']));
  });

  it('cannot start an import with nothing in it', async () => {
    const user = userEvent.setup();
    renderOrganiser();
    for (const name of ['Dashboard', 'Monorepo', 'Assets', 'Maybe', 'Standalone files']) {
      await user.click(screen.getByLabelText(`Include ${name}`));
    }
    expect(screen.getByRole('button', { name: 'Import' })).toBeDisabled();
  });

  it('cancels without emitting a plan', async () => {
    const user = userEvent.setup();
    const { onCancel, onImport } = renderOrganiser();
    await user.click(screen.getByRole('button', { name: 'Cancel' }));
    expect(onCancel).toHaveBeenCalledTimes(1);
    expect(onImport).not.toHaveBeenCalled();
  });
});

// ----------------------------------------------------------- accessibility

describe('accessibility and keyboard', () => {
  it('names every group control', () => {
    renderOrganiser();
    const section = groupSection('Assets');
    expect(within(section).getByLabelText('Include Assets')).toBeInTheDocument();
    expect(within(section).getByLabelText('Layout')).toBeInTheDocument();
    expect(within(section).getByLabelText('Kind')).toBeInTheDocument();
    expect(within(section).getByLabelText('Into')).toBeInTheDocument();
    expect(
      within(section).getByRole('button', { name: 'Remove Assets from the import' }),
    ).toBeInTheDocument();
  });

  it('is a modal dialog with a name', () => {
    renderOrganiser();
    const dialog = screen.getByRole('dialog', { name: 'Organise the import' });
    expect(dialog).toHaveAttribute('aria-modal', 'true');
  });

  it('can be walked from group to group with the keyboard', async () => {
    const user = userEvent.setup();
    renderOrganiser();
    await user.tab();
    const first = document.activeElement;
    expect(first).toBeInstanceOf(HTMLElement);

    for (let press = 0; press < 6; press += 1) await user.tab();
    expect(screen.getByRole('dialog')).toContainElement(document.activeElement as HTMLElement);
  });

  it('leaves the rename field editable without the explorer stealing the keys', async () => {
    const user = userEvent.setup();
    renderOrganiser();
    await user.dblClick(screen.getByRole('button', { name: 'Assets' }));
    const field = screen.getByLabelText('Group name');
    await user.clear(field);
    await user.type(field, 'Renamed');
    expect(field).toHaveValue('Renamed');
  });

  it('abandons a rename on Escape', async () => {
    const user = userEvent.setup();
    renderOrganiser();
    await user.dblClick(screen.getByRole('button', { name: 'Assets' }));
    await user.keyboard('{Escape}');
    expect(screen.getByRole('button', { name: 'Assets' })).toBeInTheDocument();
  });
});

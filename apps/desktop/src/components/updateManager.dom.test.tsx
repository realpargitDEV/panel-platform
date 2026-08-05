/**
 * The update manager, rendered.
 *
 * Every state is reachable here by passing it, which is the point of splitting
 * `UpdateManagerView` from the connected component: the twelve screens are
 * covered without a network, a download, or a timer, and nothing in this file
 * waits for one.
 *
 * The connected component is exercised separately at the end, against a mocked
 * `api` module — that is the boundary where the real Tauri commands live, and
 * mocking it is what lets the wiring be asserted without a desktop.
 */
import { render, screen as dom, waitFor } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';

import type { UpdateProgressEvent, UpdateScreen } from '../update';

/**
 * Hoisted, because `vi.mock` runs before the module body and a factory that
 * closed over ordinary `const`s would read them in their temporal dead zone.
 */
const backend = vi.hoisted(() => ({
  checkForUpdate: vi.fn(),
  installUpdate: vi.fn(),
  restartApp: vi.fn(),
  minimizeWindow: vi.fn(),
  emit: null as ((event: UpdateProgressEvent) => void) | null,
}));

vi.mock('../api', () => ({
  checkForUpdate: backend.checkForUpdate,
  installUpdate: backend.installUpdate,
  restartApp: backend.restartApp,
  minimizeWindow: backend.minimizeWindow,
  onUpdateProgress: (handler: (event: UpdateProgressEvent) => void) => {
    backend.emit = handler;
    return Promise.resolve(() => {
      backend.emit = null;
    });
  },
}));

import UpdateManager, { UpdateManagerView } from './UpdateManager';
import { updateStore } from '../useUpdate';

function actions() {
  return {
    onInstall: vi.fn(),
    onCheck: vi.fn(),
    onMinimize: vi.fn(),
    onRestart: vi.fn(),
    onClose: vi.fn(),
  };
}

function show(screenValue: UpdateScreen, version = '0.1.11') {
  const handlers = actions();
  render(<UpdateManagerView screen={screenValue} currentVersion={version} actions={handlers} />);
  return handlers;
}

/** One value per state, so a loop can walk all twelve. */
const EVERY_SCREEN: UpdateScreen[] = [
  { state: 'idle' },
  { state: 'checking' },
  { state: 'no_update', currentVersion: '0.1.11' },
  { state: 'ahead', currentVersion: '0.2.0', publishedVersion: '0.1.11' },
  { state: 'available', version: '0.1.12', notes: 'Fixes a crash.', publishedAt: '2026-08-01' },
  { state: 'preparing' },
  { state: 'downloading', downloadedBytes: 1_048_576, totalBytes: 4_194_304 },
  { state: 'verifying' },
  { state: 'installing' },
  { state: 'restart_required' },
  { state: 'completed' },
  { state: 'failed', message: 'connection reset', canRetry: true },
];

afterEach(() => {
  backend.checkForUpdate.mockReset();
  backend.installUpdate.mockReset();
  backend.restartApp.mockReset();
  backend.minimizeWindow.mockReset();
});

describe('every screen renders', () => {
  it('shows a dialog, a headline and the installed version in all twelve states', () => {
    for (const value of EVERY_SCREEN) {
      const { unmount } = render(
        <UpdateManagerView screen={value} currentVersion="0.1.11" actions={actions()} />,
      );
      const dialog = dom.getByRole('dialog');
      expect(dialog, value.state).toBeInTheDocument();
      expect(dialog.textContent, value.state).toContain('Panel Platform');
      expect(dialog.textContent, value.state).toContain('0.1.11');
      unmount();
    }
  });

  it('shows the release notes and the date on the available screen', () => {
    show(EVERY_SCREEN[4] as UpdateScreen);
    expect(dom.getByText('Fixes a crash.')).toBeInTheDocument();
    expect(dom.getByText(/2026-08-01/)).toBeInTheDocument();
  });

  it('does not head an empty notes panel', () => {
    show({ state: 'available', version: '0.1.12', notes: '   ', publishedAt: null });
    expect(dom.queryByText(/What's new/i)).not.toBeInTheDocument();
  });

  it('names both versions when this build is ahead of the published one', () => {
    show({ state: 'ahead', currentVersion: '0.2.0', publishedVersion: '0.1.11' });
    expect(dom.getByRole('dialog').textContent).toContain('0.2.0');
    expect(dom.getByRole('dialog').textContent).toContain('0.1.11');
  });

  it('shows the failure message itself, not a summary of it', () => {
    show({ state: 'failed', message: 'signature mismatch', canRetry: true });
    expect(dom.getByText('signature mismatch')).toBeInTheDocument();
  });
});

describe('the progress it reports', () => {
  it('reports a real percentage when the size is known', () => {
    show({ state: 'downloading', downloadedBytes: 1_048_576, totalBytes: 4_194_304 });
    const bar = dom.getByRole('progressbar');
    expect(bar).toHaveAttribute('aria-valuenow', '25');
    expect(dom.getByText('1.0 MB of 4.0 MB')).toBeInTheDocument();
    expect(dom.getByText('25%')).toBeInTheDocument();
  });

  /** A bar parked at 0% while bytes arrive reads as a hang. */
  it('reports no percentage at all when the server sent no size', () => {
    show({ state: 'downloading', downloadedBytes: 2048, totalBytes: null });
    const bar = dom.getByRole('progressbar');
    expect(bar).not.toHaveAttribute('aria-valuenow');
    expect(dom.queryByText('%')).not.toBeInTheDocument();
    expect(dom.getByText('2.0 KB downloaded')).toBeInTheDocument();
  });

  /**
   * A single sample cannot give a rate, and a rate is what an ETA is made of.
   * Neither is guessed: the first frame of every download shows no speed and no
   * time remaining, and that is correct rather than missing.
   */
  it('shows no speed and no time remaining until two samples exist', () => {
    show({ state: 'downloading', downloadedBytes: 1024, totalBytes: 4096 });
    expect(dom.queryByText(/\/s$/)).not.toBeInTheDocument();
    expect(dom.queryByText(/left$/)).not.toBeInTheDocument();
  });

  it('shows no progress bar before an install has started', () => {
    show({ state: 'available', version: '0.1.12', notes: '', publishedAt: null });
    expect(dom.queryByRole('progressbar')).not.toBeInTheDocument();
  });

  it('marks the stage the updater has actually reached', () => {
    show({ state: 'verifying' });
    const rail = dom.getByRole('list', { name: 'Update stages' });
    expect(rail.textContent).toContain('Verify');
    expect(rail.querySelector('[aria-current="step"]')).not.toBeNull();
  });
});

describe('the controls', () => {
  it('offers Install where an update is available, and nowhere else', async () => {
    const handlers = show({
      state: 'available',
      version: '0.1.12',
      notes: '',
      publishedAt: null,
    });
    await userEvent.click(dom.getByRole('button', { name: 'Install now' }));
    expect(handlers.onInstall).toHaveBeenCalledOnce();
  });

  it('offers Minimise through every wait', () => {
    for (const value of EVERY_SCREEN.filter((item) =>
      ['checking', 'preparing', 'downloading', 'verifying', 'installing'].includes(item.state),
    )) {
      const { unmount } = render(
        <UpdateManagerView screen={value} currentVersion="0.1.11" actions={actions()} />,
      );
      expect(dom.getByRole('button', { name: 'Minimise' }), value.state).toBeInTheDocument();
      unmount();
    }
  });

  it('does not offer Minimise once there is nothing to wait for', () => {
    show({ state: 'completed' });
    expect(dom.queryByRole('button', { name: 'Minimise' })).not.toBeInTheDocument();
  });

  it('minimises without disturbing the update', async () => {
    const handlers = show({ state: 'downloading', downloadedBytes: 1, totalBytes: 2 });
    await userEvent.click(dom.getByRole('button', { name: 'Minimise' }));
    expect(handlers.onMinimize).toHaveBeenCalledOnce();
    // Nothing else fired: minimising is not a way to cancel.
    expect(handlers.onClose).not.toHaveBeenCalled();
    expect(handlers.onInstall).not.toHaveBeenCalled();
  });

  it('offers Restart once the update is in place', async () => {
    for (const state of ['restart_required', 'completed'] as const) {
      const handlers = actions();
      const { unmount } = render(
        <UpdateManagerView screen={{ state }} currentVersion="0.1.11" actions={handlers} />,
      );
      await userEvent.click(dom.getByRole('button', { name: 'Restart now' }));
      expect(handlers.onRestart, state).toHaveBeenCalledOnce();
      unmount();
    }
  });

  it('does not offer Restart while the installer is still running', () => {
    show({ state: 'installing' });
    expect(dom.queryByRole('button', { name: 'Restart now' })).not.toBeInTheDocument();
  });

  it('retries the install after an install failure', async () => {
    const handlers = show({ state: 'failed', message: 'connection reset', canRetry: true });
    await userEvent.click(dom.getByRole('button', { name: 'Try again' }));
    expect(handlers.onInstall).toHaveBeenCalledOnce();
    expect(handlers.onCheck).not.toHaveBeenCalled();
  });

  /** A check that failed cannot be recovered by installing something. */
  it('retries the check after a check failure, and offers no install', async () => {
    const handlers = show({ state: 'failed', message: 'offline', canRetry: false });
    expect(dom.queryByRole('button', { name: 'Try again' })).not.toBeInTheDocument();
    await userEvent.click(dom.getByRole('button', { name: 'Check again' }));
    expect(handlers.onCheck).toHaveBeenCalledOnce();
    expect(handlers.onInstall).not.toHaveBeenCalled();
  });

  it('disables Close while closing would cost the update', () => {
    for (const state of ['preparing', 'downloading', 'verifying', 'installing'] as const) {
      const value: UpdateScreen =
        state === 'downloading' ? { state, downloadedBytes: 1, totalBytes: 2 } : { state };
      const { unmount } = render(
        <UpdateManagerView screen={value} currentVersion="0.1.11" actions={actions()} />,
      );
      // Disabled rather than absent: a missing button reads as a window that
      // cannot be closed at all.
      expect(dom.getByRole('button', { name: 'Close' }), state).toBeDisabled();
      unmount();
    }
  });

  it('allows Close everywhere it is safe', async () => {
    for (const state of ['idle', 'checking', 'no_update', 'completed'] as const) {
      const handlers = actions();
      const value: UpdateScreen =
        state === 'no_update' ? { state, currentVersion: '0.1.11' } : { state };
      const { unmount } = render(
        <UpdateManagerView screen={value} currentVersion="0.1.11" actions={handlers} />,
      );
      await userEvent.click(dom.getByRole('button', { name: 'Close' }));
      expect(handlers.onClose, state).toHaveBeenCalledOnce();
      unmount();
    }
  });

  /**
   * The architectural limitation, asserted rather than described. Tauri's
   * updater downloads, verifies and installs in one uninterruptible call, so
   * neither control could do what it says.
   */
  it('never renders a pause or a cancel', () => {
    for (const value of EVERY_SCREEN) {
      const { unmount } = render(
        <UpdateManagerView screen={value} currentVersion="0.1.11" actions={actions()} />,
      );
      expect(dom.queryByRole('button', { name: /pause/i }), value.state).toBeNull();
      expect(dom.queryByRole('button', { name: /cancel/i }), value.state).toBeNull();
      unmount();
    }
  });

  it('marks the dialog busy only while it is working', () => {
    const { unmount } = render(
      <UpdateManagerView screen={{ state: 'verifying' }} currentVersion="1" actions={actions()} />,
    );
    expect(dom.getByRole('dialog')).toHaveAttribute('aria-busy', 'true');
    unmount();

    render(
      <UpdateManagerView screen={{ state: 'completed' }} currentVersion="1" actions={actions()} />,
    );
    expect(dom.getByRole('dialog')).toHaveAttribute('aria-busy', 'false');
  });
});

describe('layout and motion', () => {
  /**
   * Motion in this application is CSS — `--motion-scale` multiplied into every
   * duration — so what a test without a stylesheet can prove is the part that
   * matters: nothing the user needs is delivered *by* an animation. With motion
   * turned off the same headline, the same progress and the same controls are
   * present.
   */
  it('renders the same content with motion off as with motion on', () => {
    const value: UpdateScreen = { state: 'downloading', downloadedBytes: 5, totalBytes: 10 };

    document.documentElement.dataset.motion = 'off';
    const off = render(
      <UpdateManagerView screen={value} currentVersion="0.1.11" actions={actions()} />,
    );
    const withoutMotion = dom.getByRole('dialog').textContent;
    expect(dom.getByRole('progressbar')).toHaveAttribute('aria-valuenow', '50');
    off.unmount();

    delete document.documentElement.dataset.motion;
    render(<UpdateManagerView screen={value} currentVersion="0.1.11" actions={actions()} />);
    expect(dom.getByRole('dialog').textContent).toBe(withoutMotion);
    expect(dom.getByRole('progressbar')).toHaveAttribute('aria-valuenow', '50');
  });

  /**
   * jsdom does no layout, so this is a structural check rather than a visual
   * one: the dialog is width-capped and scrolls its own body, and the figures
   * under the bar wrap instead of truncating. Whether that *looks* right at a
   * given size is not something this suite can claim.
   */
  it('is built to survive a narrow window', () => {
    show({ state: 'downloading', downloadedBytes: 5, totalBytes: 10 });
    const dialog = dom.getByRole('dialog');
    expect(dialog.className).toContain('max-w-[520px]');
    expect(dialog.className).toContain('max-h-full');
    expect(dialog.querySelector('.overflow-y-auto')).not.toBeNull();
    expect(dialog.querySelector('.flex-wrap')).not.toBeNull();
  });
});

/**
 * The store is a module singleton — there is one updater per window, which is
 * the whole reason it exists — so these run in order against one instance, and
 * the order is the point: a failure is recovered from *before* the successful
 * install, because `installed` is terminal by design and nothing after it can
 * start again without a restart.
 */
describe('the connected window', () => {
  const availableCheck = {
    state: 'available',
    currentVersion: '0.1.11',
    newVersion: '0.1.12',
    notes: 'Fixes',
    publishedAt: null,
    downloadUrl: 'https://example.test/x',
    signature: 'sig',
  };

  it('renders nothing until it is opened', () => {
    render(<UpdateManager open={false} currentVersion="0.1.11" onClose={() => {}} />);
    expect(dom.queryByRole('dialog')).not.toBeInTheDocument();
  });

  it('recovers from a failed install by retrying it, then by checking again', async () => {
    backend.checkForUpdate.mockResolvedValue(availableCheck);
    backend.installUpdate.mockRejectedValue(new Error('connection reset'));

    render(<UpdateManager open currentVersion="0.1.11" onClose={() => {}} />);
    await updateStore.check();
    await waitFor(() => expect(dom.getByText(/0\.1\.12 is available/)).toBeInTheDocument());

    await updateStore.install();
    await waitFor(() => expect(dom.getByText(/connection reset/)).toBeInTheDocument());
    // The intent is spent, or the retry below would show `preparing` forever.
    expect(updateStore.getState().accepted).toBe(false);

    await userEvent.click(dom.getByRole('button', { name: 'Try again' }));
    await waitFor(() => expect(backend.installUpdate).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(dom.getByText(/connection reset/)).toBeInTheDocument());

    // A fresh check supersedes the stale failure and returns to the offer.
    await userEvent.click(dom.getByRole('button', { name: 'Check again' }));
    await waitFor(() => expect(dom.getByText(/0\.1\.12 is available/)).toBeInTheDocument());
  });

  it('drives the whole install from the store, and finishes on a real restart', async () => {
    backend.checkForUpdate.mockResolvedValue(availableCheck);
    backend.minimizeWindow.mockResolvedValue(undefined);
    backend.restartApp.mockResolvedValue(undefined);
    // `let finish: (() => void) | null` assigned inside the executor is
    // narrowed to `never` by control-flow analysis, hence the definite
    // assignment rather than the obvious spelling.
    let finish!: () => void;
    const installing = new Promise<void>((resolve) => {
      finish = resolve;
    });
    backend.installUpdate.mockImplementation(() => installing);

    render(<UpdateManager open currentVersion="0.1.11" onClose={() => {}} />);
    await updateStore.check();
    await waitFor(() => expect(dom.getByText(/0\.1\.12 is available/)).toBeInTheDocument());

    await userEvent.click(dom.getByRole('button', { name: 'Install now' }));
    // Preparing, because no byte has been reported yet — not "downloading 0%".
    await waitFor(() => expect(dom.getByText('Preparing the download')).toBeInTheDocument());
    expect(dom.getByRole('button', { name: 'Close' })).toBeDisabled();

    backend.emit?.({ phase: 'downloading', downloadedBytes: 512, totalBytes: 1024 });
    await waitFor(() =>
      expect(dom.getByRole('progressbar')).toHaveAttribute('aria-valuenow', '50'),
    );

    // Minimising a running download must not disturb it.
    await userEvent.click(dom.getByRole('button', { name: 'Minimise' }));
    expect(backend.minimizeWindow).toHaveBeenCalledOnce();
    expect(dom.getByRole('progressbar')).toHaveAttribute('aria-valuenow', '50');

    backend.emit?.({ phase: 'verifying', downloadedBytes: 1024, totalBytes: 1024 });
    await waitFor(() => expect(dom.getByText('Verifying the download')).toBeInTheDocument());

    backend.emit?.({ phase: 'installing', downloadedBytes: 1024, totalBytes: 1024 });
    await waitFor(() => expect(dom.getByText('Installing the update')).toBeInTheDocument());

    finish();
    await waitFor(() => expect(dom.getByText('Update installed')).toBeInTheDocument());
    expect(dom.getByRole('button', { name: 'Close' })).toBeEnabled();
    expect(updateStore.getState().accepted).toBe(false);

    await userEvent.click(dom.getByRole('button', { name: 'Restart now' }));
    expect(backend.restartApp).toHaveBeenCalledOnce();
  });
});

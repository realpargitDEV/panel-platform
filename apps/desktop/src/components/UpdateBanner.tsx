import { useEffect, useState } from 'react';
import { checkForUpdate, installUpdate, type UpdateCheck } from '../api';
import { buttonLabel, canStart, failureMessage, idle, type InstallPhase } from '../update';

/**
 * The update prompt.
 *
 * Checked once when the window opens. A failure to *check* is deliberately
 * silent — being unable to reach GitHub is not worth interrupting anyone over,
 * and the application is designed to run with no internet at all. The Settings
 * screen has an explicit check that does report failures.
 *
 * A failure to *install* is never silent: the user asked for that one.
 */
export default function UpdateBanner() {
  const [update, setUpdate] = useState<UpdateCheck | null>(null);
  const [dismissed, setDismissed] = useState(false);
  const [phase, setPhase] = useState<InstallPhase>(idle);

  useEffect(() => {
    checkForUpdate()
      .then(setUpdate)
      .catch(() => setUpdate(null));
  }, []);

  if (dismissed || update === null || update.state !== 'available') {
    return null;
  }

  async function install() {
    setPhase({ state: 'installing' });
    try {
      await installUpdate();
      // Reached on Linux only. On Windows the installer takes over and this
      // process is gone before the await returns.
      setPhase({ state: 'installed' });
    } catch (error) {
      setPhase({ state: 'failed', message: failureMessage(error) });
    }
  }

  return (
    <div className="bg-accent px-8 py-2.5 text-white">
      <div className="flex flex-wrap items-center gap-3">
        <p className="flex-1 text-sm font-medium">
          There is an update available — version {update.newVersion}
        </p>
        <button
          type="button"
          onClick={() => void install()}
          disabled={!canStart(phase)}
          className="rounded-md bg-white/20 px-3 py-1.5 text-sm font-medium hover:bg-white/30 disabled:cursor-not-allowed disabled:opacity-60"
        >
          {buttonLabel(phase)}
        </button>
        <button
          type="button"
          onClick={() => setDismissed(true)}
          className="rounded-md px-3 py-1.5 text-sm font-medium hover:bg-white/10"
        >
          Later
        </button>
      </div>

      {phase.state === 'failed' && <p className="mt-1.5 text-sm text-white/90">{phase.message}</p>}
      {phase.state === 'installed' && (
        <p className="mt-1.5 text-sm text-white/90">
          Installed. Close and reopen Panel Platform to finish.
        </p>
      )}
    </div>
  );
}

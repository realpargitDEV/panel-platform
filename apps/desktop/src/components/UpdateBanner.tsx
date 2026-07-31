import { useEffect, useState } from 'react';
import { buttonLabel, canStart } from '../update';
import { updateStore, useUpdate } from '../useUpdate';
import UpdateProgress from './UpdateProgress';

/**
 * The update prompt.
 *
 * Checking starts here because this is mounted for the life of the window: once
 * at startup, then every few hours. A failure to *check* is deliberately silent
 * — being unable to reach GitHub is not worth interrupting anyone over, and the
 * application is designed to run with no internet at all. The Settings screen
 * has an explicit check that does report failures.
 *
 * A failure to *install* is never silent: the user asked for that one.
 */
export default function UpdateBanner() {
  const { check, phase } = useUpdate();
  const [dismissed, setDismissed] = useState(false);

  useEffect(() => {
    updateStore.start();
    // Deliberately not stopped on unmount: this component lives as long as the
    // window does, and tearing the timer down here would end periodic checking
    // the first time React remounted it in development.
  }, []);

  // Still shown while an install is running even if the user pressed Later:
  // work in progress must not vanish behind a dismissed banner.
  const busy = phase.state !== 'idle';
  if ((dismissed && !busy) || check === null || check.state !== 'available') {
    return null;
  }

  return (
    <div className="bg-accent px-8 py-2.5 text-white">
      <div className="flex flex-wrap items-center gap-3">
        <p className="flex-1 text-sm font-medium">Update available — version {check.newVersion}</p>
        <button
          type="button"
          onClick={() => void updateStore.install()}
          disabled={!canStart(phase)}
          className="rounded-md bg-white/20 px-3 py-1.5 text-sm font-medium hover:bg-white/30 disabled:cursor-not-allowed disabled:opacity-60"
        >
          {buttonLabel(phase)}
        </button>
        {!busy && (
          <button
            type="button"
            onClick={() => setDismissed(true)}
            className="rounded-md px-3 py-1.5 text-sm font-medium hover:bg-white/10"
          >
            Later
          </button>
        )}
      </div>

      <UpdateProgress phase={phase} tone="banner" />
    </div>
  );
}

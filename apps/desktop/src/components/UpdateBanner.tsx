import { useEffect, useState } from 'react';
import { checkForUpdate, type UpdateCheck } from '../api';

/**
 * The update prompt.
 *
 * Checked once when the window opens. A failure is deliberately silent — being
 * unable to reach GitHub is not worth interrupting anyone over, and the
 * application is designed to run with no internet at all. The Settings screen
 * has an explicit check that does report failures.
 */
export default function UpdateBanner() {
  const [update, setUpdate] = useState<UpdateCheck | null>(null);
  const [dismissed, setDismissed] = useState(false);

  useEffect(() => {
    checkForUpdate()
      .then(setUpdate)
      .catch(() => setUpdate(null));
  }, []);

  if (dismissed || update === null || update.state !== 'available') {
    return null;
  }

  return (
    <div className="flex flex-wrap items-center gap-3 bg-accent px-8 py-2.5 text-white">
      <p className="flex-1 text-sm font-medium">
        There is an update available — version {update.newVersion}
      </p>
      <button
        type="button"
        disabled
        title="Installing updates is not implemented yet"
        className="rounded-md bg-white/20 px-3 py-1.5 text-sm font-medium disabled:cursor-not-allowed disabled:opacity-60"
      >
        Update now
      </button>
      <button
        type="button"
        onClick={() => setDismissed(true)}
        className="rounded-md px-3 py-1.5 text-sm font-medium hover:bg-white/10"
      >
        Later
      </button>
    </div>
  );
}

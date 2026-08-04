/**
 * The update manager.
 *
 * One component because the banner and the Settings screen must not drift into
 * describing the same install differently; `tone` is the only difference.
 *
 * # What is real here, and what is deliberately absent
 *
 * Every figure on screen is derived from bytes the backend actually reported —
 * percentage, size, rate and time remaining all come from `update.ts`, and each
 * of them renders nothing rather than a guess when the data cannot support one.
 *
 * There is **no pause and no cancel**. The updater downloads and installs in
 * one uninterruptible call, so both controls would be buttons that do not do
 * what they say. A resumable transfer would be needed first, and inventing the
 * buttons before the capability is exactly the placeholder this screen is
 * supposed to replace.
 *
 * `Retry` is real: it re-enters the same install. `Restart` is not offered,
 * because nothing in the core relaunches the application — so the finished
 * state says what to do instead of pretending to do it.
 */
import { useEffect, useRef, useState } from 'react';

import {
  formatBytes,
  formatRate,
  formatRemaining,
  progressCaption,
  progressPercent,
  rateBetween,
  secondsRemaining,
  smoothRate,
  type InstallPhase,
} from '../update';
import { updateStore } from '../useUpdate';
import Icon, { type IconName } from '../ui/Icon';
import { Button } from '../ui/primitives';

/** The stages, in the order they happen. Shown as a rail so the user can see
 *  where they are in a process that takes minutes, not where they are in a
 *  single bar. */
const STAGES: { id: InstallPhase['state']; label: string; icon: IconName }[] = [
  { id: 'downloading', label: 'Download', icon: 'download' },
  { id: 'verifying', label: 'Verify', icon: 'shield' },
  { id: 'installing', label: 'Install', icon: 'container' },
  { id: 'restarting', label: 'Finish', icon: 'check-circle' },
];

function stageIndex(phase: InstallPhase): number {
  const found = STAGES.findIndex((stage) => stage.id === phase.state);
  if (found >= 0) return found;
  return phase.state === 'installed' ? STAGES.length : -1;
}

/**
 * Track how fast the download is going.
 *
 * Kept here rather than in the store because it is a property of *watching* a
 * download, not of the download: a second window opening halfway through should
 * start its own estimate rather than inherit one built from bytes it never saw.
 */
function useTransferRate(phase: InstallPhase): number | null {
  const [rate, setRate] = useState<number | null>(null);
  const previous = useRef<{ at: number; bytes: number } | null>(null);

  const bytes = phase.state === 'downloading' ? phase.downloadedBytes : null;

  useEffect(() => {
    if (bytes === null) {
      previous.current = null;
      setRate(null);
      return;
    }

    const sample = { at: performance.now(), bytes };
    const last = previous.current;
    previous.current = sample;
    if (!last) return;

    const measured = rateBetween(last, sample);
    if (measured !== null) setRate((current) => smoothRate(current, measured));
  }, [bytes]);

  return rate;
}

export default function UpdateProgress({
  phase,
  tone,
}: {
  phase: InstallPhase;
  tone: 'banner' | 'panel';
}) {
  const percent = progressPercent(phase);
  const caption = progressCaption(phase);
  const rate = useTransferRate(phase);
  const banner = tone === 'banner';

  const muted = banner ? 'text-white/85' : 'text-muted';
  const track = banner ? 'bg-white/25' : 'bg-raised';
  const fill = banner ? 'bg-white' : 'bg-accent';

  // ------------------------------------------------------------- failed
  if (phase.state === 'failed') {
    return (
      <div className={`animate-pop mt-2 ${banner ? '' : 'rounded-[10px] bg-danger-soft p-3'}`}>
        <div className="flex items-start gap-2">
          <span className={banner ? 'text-white' : 'text-danger'}>
            <Icon name="alert" size={15} />
          </span>
          <div className="min-w-0 flex-1">
            <p className={`text-[13px] ${banner ? 'text-white' : 'text-danger'}`}>
              The update did not finish
            </p>
            <p className={`mt-0.5 text-[12px] ${muted}`}>{phase.message}</p>
          </div>
        </div>
        {!banner && (
          <div className="mt-3 flex flex-wrap gap-2">
            <Button size="sm" icon="refresh" onClick={() => void updateStore.install()}>
              Try again
            </Button>
            <Button
              size="sm"
              variant="ghost"
              icon="refresh"
              onClick={() => void updateStore.check()}
            >
              Check again
            </Button>
          </div>
        )}
      </div>
    );
  }

  // ---------------------------------------------------------- installed
  if (phase.state === 'installed') {
    return (
      <div className={`animate-pop mt-2 ${banner ? '' : 'rounded-[10px] bg-ok-soft p-3'}`}>
        <div className="flex items-start gap-2">
          <span className={banner ? 'text-white' : 'text-ok'}>
            <Icon name="check-circle" size={15} />
          </span>
          <div className="min-w-0">
            <p className={`text-[13px] ${banner ? 'text-white' : 'text-ok'}`}>Update installed</p>
            {/* Not a Restart button: nothing in the core relaunches the
                application, so this says what to do rather than offering a
                control that would do nothing. */}
            <p className={`mt-0.5 text-[12px] ${muted}`}>
              Close and reopen Panel Platform to start the new version.
            </p>
          </div>
        </div>
      </div>
    );
  }

  if (caption === null) return null;

  const active = stageIndex(phase);
  const remaining =
    phase.state === 'downloading'
      ? formatRemaining(secondsRemaining(phase.downloadedBytes, phase.totalBytes, rate))
      : null;
  const speed = phase.state === 'downloading' ? formatRate(rate) : null;

  return (
    <div className="mt-2">
      {/* The rail. Hidden in the banner, which has one line to work with. */}
      {!banner && (
        <ol className="mb-2.5 flex items-center gap-1.5">
          {STAGES.map((stage, index) => {
            const done = index < active;
            const current = index === active;
            return (
              <li key={stage.id} className="flex min-w-0 flex-1 items-center gap-1.5">
                <span
                  className={`grid h-5 w-5 shrink-0 place-items-center rounded-full ${
                    done
                      ? 'bg-accent text-white'
                      : current
                        ? 'bg-accent-soft text-accent'
                        : 'bg-raised text-faint'
                  }`}
                >
                  <Icon
                    name={done ? 'check' : stage.icon}
                    size={11}
                    className={current ? 'animate-mark' : undefined}
                  />
                </span>
                <span
                  className={`truncate text-[11px] ${current ? 'text-ink' : done ? 'text-muted' : 'text-faint'}`}
                >
                  {stage.label}
                </span>
                {index < STAGES.length - 1 && (
                  <span
                    aria-hidden
                    className={`h-px min-w-2 flex-1 ${done ? 'bg-accent' : 'bg-edge'}`}
                  />
                )}
              </li>
            );
          })}
        </ol>
      )}

      <div className={`h-1.5 w-full overflow-hidden rounded-full ${track}`}>
        <div
          className={`h-full rounded-full ${fill} ${
            percent === null ? 'w-1/3 animate-pulse' : 'transition-[width] duration-200'
          }`}
          style={percent === null ? undefined : { width: `${percent}%` }}
          role="progressbar"
          aria-valuenow={percent ?? undefined}
          aria-valuemin={0}
          aria-valuemax={100}
          aria-label="Update progress"
        />
      </div>

      {/* Wraps rather than truncating: on a narrow window the rate and the time
          left drop to a second line instead of disappearing. */}
      <div className={`mt-1.5 flex flex-wrap items-center gap-x-2 gap-y-0.5 text-[12px] ${muted}`}>
        <span className="text-ink">{caption}</span>
        {percent !== null && <span className="tabular">{percent}%</span>}
        {speed && <span className="tabular">{speed}</span>}
        {remaining && <span className="tabular">{remaining}</span>}
        {phase.state === 'downloading' && phase.totalBytes !== null && (
          <span className="tabular sr-only">{formatBytes(phase.totalBytes)} total</span>
        )}
      </div>
    </div>
  );
}

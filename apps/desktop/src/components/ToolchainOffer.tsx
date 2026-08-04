/**
 * What the machine is missing, and what would be done about it.
 *
 * Shown before anything runs. Every command is listed in full rather than
 * summarised as "install dependencies": the user is about to approve an
 * elevation prompt, and approving one without seeing what it covers is the
 * thing this dialog exists to prevent.
 */
import { useEffect, useState } from 'react';

import { onToolchainProgress, type ToolchainProgress, type ToolchainStep } from '../api';
import { Modal } from '../ui/overlays';
import { Button } from '../ui/primitives';

export default function ToolchainOffer({
  displayName,
  steps,
  needsElevation,
  installing,
  onInstall,
  onCancel,
}: {
  displayName: string;
  steps: ToolchainStep[];
  needsElevation: boolean;
  installing: boolean;
  onInstall: () => void;
  onCancel: () => void;
}) {
  const [progress, setProgress] = useState<ToolchainProgress | null>(null);

  useEffect(() => {
    let stop: (() => void) | null = null;
    let cancelled = false;

    void onToolchainProgress((next) => setProgress(next)).then((unlisten) => {
      // The dialog can close before the listener resolves; without this the
      // unlisten is dropped and the handler outlives the component.
      if (cancelled) unlisten();
      else stop = unlisten;
    });

    return () => {
      cancelled = true;
      stop?.();
    };
  }, []);

  return (
    <Modal
      title={`${displayName} is not installed on this computer`}
      description={`Starting this project needs ${displayName}. Panel Platform can install it for you.`}
      size="md"
      onClose={installing ? () => undefined : onCancel}
      footer={
        <>
          <Button onClick={onCancel} disabled={installing}>
            Cancel
          </Button>
          <Button variant="primary" onClick={onInstall} disabled={installing}>
            {installing ? 'Installing…' : 'Install and start'}
          </Button>
        </>
      }
    >
      <ol className="flex flex-col gap-2">
        {steps.map((step, index) => {
          const done = progress !== null && progress.step > index + 1;
          const active = progress !== null && progress.step === index + 1;

          return (
            <li
              key={`${step.describes}-${index}`}
              className="flex items-start gap-2 text-[13px]"
              aria-current={active ? 'step' : undefined}
            >
              <span aria-hidden className="mt-[2px] w-4 shrink-0 text-center text-muted">
                {done ? '✓' : active ? '›' : index + 1}
              </span>
              <span className={done ? 'text-muted line-through' : undefined}>
                {step.describes}
                {step.elevated && (
                  <span className="ml-2 text-[11px] text-muted">needs administrator</span>
                )}
              </span>
            </li>
          );
        })}
      </ol>

      {needsElevation && (
        <p className="mt-3 text-[13px] text-muted">
          Windows will ask for permission before each step marked <em>needs administrator</em>.
          Panel Platform itself keeps running normally — only the installer is elevated, and your
          project never runs with those rights.
        </p>
      )}

      {progress && (
        <p className="mt-3 text-[13px]" role="status">
          Step {progress.step} of {progress.of}: {progress.describes}
        </p>
      )}
    </Modal>
  );
}

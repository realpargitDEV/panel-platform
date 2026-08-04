/**
 * The check that stands between pressing Start and the project starting.
 *
 * A hook rather than something each page implements, because a project can be
 * started from five places — the list, the detail page, the command palette,
 * the workspace, and straight after creation — and a check wired to only some
 * of them is worse than none: the same project would behave differently
 * depending on which button was pressed.
 *
 * Callers wrap the action rather than branch on a result:
 *
 * ```ts
 * const { gate, guard } = useToolchainGate();
 * onAct(project, 'started', guard(startProject));
 * ```
 *
 * `guard` refuses by throwing, so an existing catch reports it, and their
 * success toast never fires for a start that did not happen. Declining is not
 * a failure, so it throws {@link ToolchainDeclined}, which those handlers
 * swallow.
 */
import { useCallback, useRef, useState } from 'react';

import {
  errorMessage,
  installToolchain,
  toolchainReadiness,
  type ToolchainReadiness,
} from '../api';
import { toast } from '../ui/toast';
import ToolchainOffer from './ToolchainOffer';

/** The user was offered an install and said no. Not an error to report. */
export class ToolchainDeclined extends Error {
  constructor() {
    super('The install was declined.');
    this.name = 'ToolchainDeclined';
  }
}

type Offer = Extract<ToolchainReadiness, { state: 'needs_install' }> & { projectId: string };

export function useToolchainGate() {
  const [offer, setOffer] = useState<Offer | null>(null);
  const [installing, setInstalling] = useState(false);
  /** Resolves the promise `guard` is waiting on once the user has answered. */
  const pending = useRef<((ready: boolean) => void) | null>(null);

  const settle = useCallback((ready: boolean) => {
    setOffer(null);
    pending.current?.(ready);
    pending.current = null;
  }, []);

  const ensureReady = useCallback(async (projectId: string): Promise<boolean> => {
    const readiness = await toolchainReadiness(projectId);

    if (readiness.state === 'ready') return true;

    // Thrown rather than returned: a blocked machine is a real failure with a
    // message worth showing, and the caller's own error handling says it.
    if (readiness.state === 'blocked') throw new Error(readiness.message);

    return new Promise<boolean>((resolve) => {
      pending.current = resolve;
      setOffer({ ...readiness, projectId });
    });
  }, []);

  /** Wrap a start action so it runs only once the machine can support it. */
  const guard = useCallback(
    (action: (id: string) => Promise<unknown>) => async (projectId: string) => {
      if (!(await ensureReady(projectId))) throw new ToolchainDeclined();
      return action(projectId);
    },
    [ensureReady],
  );

  async function accept() {
    if (!offer) return;
    setInstalling(true);
    try {
      await installToolchain(offer.projectId);
      settle(true);
    } catch (error) {
      toast.error('The install did not finish', errorMessage(error));
      settle(false);
    } finally {
      setInstalling(false);
    }
  }

  const gate = offer ? (
    <ToolchainOffer
      displayName={offer.display_name}
      steps={offer.steps}
      needsElevation={offer.needs_elevation}
      installing={installing}
      onInstall={() => void accept()}
      onCancel={() => settle(false)}
    />
  ) : null;

  return { gate, guard };
}

/** Whether an error is a user declining, which no toast should report. */
export function isDeclined(error: unknown): boolean {
  return error instanceof ToolchainDeclined;
}

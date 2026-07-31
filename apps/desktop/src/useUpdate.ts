/**
 * The one update process, shared by everything that shows it.
 *
 * A module-level store rather than a context provider: there is exactly one
 * updater for the life of the window, it is not parameterised by anything, and
 * a provider would only add a place to forget to mount it.
 */
import { useSyncExternalStore } from 'react';

import { checkForUpdate, installUpdate, onUpdateProgress } from './api';
import { createUpdateStore, type UpdateState } from './update';

export const updateStore = createUpdateStore({
  check: checkForUpdate,
  install: installUpdate,
  onProgress: onUpdateProgress,
});

/** The current update state, re-rendering the caller when it changes. */
export function useUpdate(): UpdateState {
  return useSyncExternalStore(updateStore.subscribe, updateStore.getState, updateStore.getState);
}

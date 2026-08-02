/**
 * Replacing existing files without ever having none of them.
 *
 * "Replace" is the one resolution that destroys something, so it is the one
 * that has to be transactional. The order is fixed and it matters:
 *
 *   1. Rename the existing item aside. Nothing is lost yet.
 *   2. Copy the incoming files in.
 *   3. Only now delete what was moved aside.
 *
 * If anything fails between 1 and 3, everything this operation created is
 * removed and everything it moved aside is renamed back — so a failed replace
 * leaves exactly what was there before, not a hole. Deleting first and copying
 * second would be one line shorter and would lose the user's file whenever the
 * copy failed, which is precisely the case the user is most likely to hit.
 *
 * The filesystem is injected. That is not for tidiness: rollback is the code
 * that only runs when something has already gone wrong, so the only way to know
 * it works is to make the failure happen on purpose. A test that cannot fail
 * step 2 cannot prove step 3 was skipped.
 */
import type { ImportOperationError, ImportPhase, OperationBatch } from './importOperation';

/** One thing moved aside, and how to put it back. */
export interface StagedItem {
  /** Where it is now, out of the way. */
  backup: string;
  /** Where it was, and where it goes back to. */
  original: string;
  wasDirectory: boolean;
}

/**
 * The filesystem, as this transaction needs it.
 *
 * Deliberately four operations and no more. Anything richer would let the
 * transaction reach for a shortcut that the real backend does not offer.
 */
export interface TransactionIo {
  /** Rename within the same directory; the core refuses a separator here. */
  rename(path: string, toName: string): Promise<void>;
  remove(path: string, isDirectory: boolean): Promise<void>;
  /** Run one batch, returning the top-level paths it created. */
  importBatch(batch: OperationBatch): Promise<string[]>;
  /** What is at a path right now, so a delete knows what it is deleting. */
  isDirectory(path: string): boolean;
}

export interface TransactionHooks {
  onPhase?(phase: ImportPhase): void;
  onBatchComplete?(batch: OperationBatch): void;
  onRollbackStart?(total: number): void;
  onRollbackStep?(failure?: ImportOperationError): void;
  /** Asked before each batch, so a cancellation stops at a safe point. */
  isCancelled?(): boolean;
}

export interface TransactionResult {
  outcome: 'completed' | 'failed' | 'cancelled';
  /** Top-level paths the import created, in the order they appeared. */
  created: string[];
  /** What was moved aside, and whether it was put back. */
  staged: StagedItem[];
  /** The first failure, which is the one worth reporting. */
  failure: string | null;
  /**
   * Everything that could not be undone, with the exact path. A rollback
   * failure is the only outcome where the user must go and look themselves, so
   * "something went wrong" is not good enough — the path has to be in the text.
   */
  errors: ImportOperationError[];
}

/** The last component of a project-relative path. */
function nameOf(path: string): string {
  const cut = path.lastIndexOf('/');
  return cut < 0 ? path : path.slice(cut + 1);
}

function messageOf(error: unknown): string {
  return error instanceof Error ? error.message : String(error);
}

/**
 * Carry out an organised import, staging anything it replaces.
 *
 * `backupSuffix` keeps the moved-aside copy beside its original — the core
 * refuses a rename containing a separator, so it cannot be put anywhere else.
 */
export async function runReplacementTransaction(
  batches: OperationBatch[],
  io: TransactionIo,
  hooks: TransactionHooks = {},
  backupSuffix = 'replaced',
): Promise<TransactionResult> {
  const staged: StagedItem[] = [];
  const created: string[] = [];
  const errors: ImportOperationError[] = [];
  let failure: string | null = null;
  let cancelled = false;

  for (const batch of batches) {
    if (hooks.isCancelled?.() === true) {
      cancelled = true;
      break;
    }

    // Stage everything this batch replaces before it copies a single byte. A
    // batch that stages three of four and then fails must still be undoable,
    // which is why `staged` is appended to as each rename succeeds rather than
    // at the end.
    if (batch.replacePaths.length > 0) {
      hooks.onPhase?.('staging-replacements');
      for (const path of batch.replacePaths) {
        const wasDirectory = io.isDirectory(path);
        const backup = `${path}.${backupSuffix}`;
        try {
          await io.rename(path, nameOf(backup));
          staged.push({ backup, original: path, wasDirectory });
        } catch (error) {
          failure = `${path}: ${messageOf(error)}`;
          break;
        }
      }
      if (failure !== null) break;
    }

    hooks.onPhase?.('copying');
    try {
      for (const path of await io.importBatch(batch)) created.push(path);
      hooks.onBatchComplete?.(batch);
    } catch (error) {
      failure = `${batch.groupName}: ${messageOf(error)}`;
      break;
    }
  }

  if (failure !== null || cancelled) {
    // Undo, newest first: remove what this operation created, then put back
    // what it moved aside. Anything that was already here and untouched is
    // never considered, which is what stops a rollback deleting the user's
    // other files.
    hooks.onRollbackStart?.(created.length + staged.length);

    for (const path of created) {
      try {
        await io.remove(path, io.isDirectory(path));
        hooks.onRollbackStep?.();
      } catch (error) {
        const entry = {
          path,
          message: `could not be removed: ${messageOf(error)}`,
          rollback: true,
        };
        errors.push(entry);
        hooks.onRollbackStep?.(entry);
      }
    }

    for (const entry of [...staged].reverse()) {
      try {
        await io.rename(entry.backup, nameOf(entry.original));
        hooks.onRollbackStep?.();
      } catch (error) {
        const failed = {
          path: entry.original,
          message: `the original is still on disk at ${entry.backup} (${messageOf(error)})`,
          rollback: true,
        };
        errors.push(failed);
        hooks.onRollbackStep?.(failed);
      }
    }

    return {
      outcome: cancelled && failure === null ? 'cancelled' : 'failed',
      created,
      staged,
      failure,
      errors,
    };
  }

  // Committed. Only now may the replaced originals go: until this point every
  // one of them is still on disk under its backup name.
  hooks.onPhase?.('committing');
  for (const entry of staged) {
    try {
      await io.remove(entry.backup, entry.wasDirectory);
    } catch (error) {
      // The import succeeded; a leftover backup is litter, not a failure, but
      // the user is told where it is.
      errors.push({
        path: entry.backup,
        message: `the replaced item could not be cleared away: ${messageOf(error)}`,
      });
    }
  }

  return { outcome: 'completed', created, staged, failure: null, errors };
}

/**
 * The new-project form's rules.
 *
 * Kept apart from the dialog because these are the parts that go quietly wrong:
 * a field that validates only on the step it lives on, a Create button enabled
 * with a required field empty, and — the one that loses work — a form that
 * discards what was typed when the source type changes.
 *
 * The draft holds every field for every source. Switching from a git clone to
 * an archive keeps the URL, the name and the description exactly where they
 * were, and only the fields that stop applying are ignored when the request is
 * built.
 */
import type { NewProjectRequest, SourceKind } from '../api';

export type StepId = 'source' | 'details' | 'runtime' | 'review';

export const STEPS: { id: StepId; title: string; description: string }[] = [
  { id: 'source', title: 'Source', description: 'Where the files come from' },
  { id: 'details', title: 'Details', description: 'Name and description' },
  { id: 'runtime', title: 'Runtime', description: 'How it is built and started' },
  { id: 'review', title: 'Review', description: 'Check and create' },
];

export interface Draft {
  source: SourceKind;
  name: string;
  description: string;
  /** Null means "let the core decide by looking at the files". */
  runtime: string | null;
  url: string;
  gitRef: string;
  subdirectory: string;
  token: string;
  /** Start the project as soon as it is created. */
  startNow: boolean;
}

export const emptyDraft: Draft = {
  source: 'EMPTY',
  name: '',
  description: '',
  runtime: null,
  url: '',
  gitRef: '',
  subdirectory: '',
  token: '',
  startNow: false,
};

/** True for the sources that fetch files from somewhere. */
export function isRemote(source: SourceKind): boolean {
  return source !== 'EMPTY';
}

/** True for the sources that understand a branch and a subdirectory. */
export function isGitLike(source: SourceKind): boolean {
  return source === 'GIT_CLONE' || source === 'GITHUB_CLI';
}

/** The GitHub CLI takes its credential from `gh`, so it has no token field. */
export function takesToken(source: SourceKind): boolean {
  return source === 'GIT_CLONE' || source === 'REMOTE_ARCHIVE';
}

export type Errors = Partial<Record<'name' | 'url' | 'runtime', string>>;

/**
 * What is wrong with the draft right now.
 *
 * Everything is checked at once rather than per step, so the Review step can
 * point at a problem three steps back instead of letting Create fail.
 */
export function validate(draft: Draft): Errors {
  const errors: Errors = {};

  const name = draft.name.trim();
  if (name.length === 0) {
    errors.name = 'A name is required.';
  } else if (name.length > 60) {
    errors.name = 'Use 60 characters or fewer.';
  }

  if (isRemote(draft.source)) {
    const url = draft.url.trim();
    if (url.length === 0) {
      errors.url =
        draft.source === 'GITHUB_CLI'
          ? 'Name the repository as owner/repo.'
          : 'An address is required.';
    } else if (draft.source === 'GITHUB_CLI') {
      // `owner/repo`, or a github.com URL. The core checks it properly; this
      // catches the obvious mistake before a round trip.
      const looksLikeUrl = /^https?:\/\//i.test(url);
      if (!looksLikeUrl && !/^[\w.-]+\/[\w.-]+$/.test(url)) {
        errors.url = 'Use owner/repo, or a github.com address.';
      }
    } else if (!/^https:\/\//i.test(url)) {
      errors.url = 'The address must start with https://.';
    }
  }

  // An empty project has no files to read, so the core cannot detect anything.
  if (!isRemote(draft.source) && draft.runtime === null) {
    errors.runtime = 'Choose a runtime — an empty project has no files to detect from.';
  }

  return errors;
}

/** Which step a given error belongs to, so Review can send the user back. */
export function stepForError(field: keyof Errors): StepId {
  switch (field) {
    case 'url':
      return 'source';
    case 'name':
      return 'details';
    default:
      return 'runtime';
  }
}

/** True when nothing on this step is wrong, so Next can be enabled. */
export function stepIsValid(draft: Draft, step: StepId): boolean {
  const errors = validate(draft);
  switch (step) {
    case 'source':
      return errors.url === undefined;
    case 'details':
      return errors.name === undefined;
    case 'runtime':
      return errors.runtime === undefined;
    case 'review':
      return Object.keys(errors).length === 0;
  }
}

export function canCreate(draft: Draft): boolean {
  return Object.keys(validate(draft)).length === 0;
}

/**
 * The request the core receives.
 *
 * Fields that do not apply to the chosen source are left out rather than sent
 * empty: the core would otherwise have to reconcile a git ref with an archive,
 * and an empty string is not the same as "not given".
 */
export function toRequest(draft: Draft): NewProjectRequest {
  const remote = isRemote(draft.source);
  const trimmed = (value: string) => {
    const result = value.trim();
    return result.length > 0 ? result : undefined;
  };

  return {
    displayName: draft.name.trim(),
    description: draft.description.trim(),
    runtime: draft.runtime ?? undefined,
    source: {
      kind: draft.source,
      url: remote ? draft.url.trim() : undefined,
      gitRef: isGitLike(draft.source) ? trimmed(draft.gitRef) : undefined,
      subdirectory: isGitLike(draft.source) ? trimmed(draft.subdirectory) : undefined,
      token: takesToken(draft.source) ? trimmed(draft.token) : undefined,
    },
  };
}

/** How long the create step is expected to take, in words. */
export function progressLabel(source: SourceKind): string {
  switch (source) {
    case 'GIT_CLONE':
    case 'GITHUB_CLI':
      return 'Cloning the repository…';
    case 'REMOTE_ARCHIVE':
      return 'Downloading and extracting…';
    default:
      return 'Creating the project…';
  }
}

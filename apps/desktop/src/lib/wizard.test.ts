import { describe, expect, it } from 'vitest';

import {
  canCreate,
  emptyDraft,
  isGitLike,
  isRemote,
  progressLabel,
  stepForError,
  stepIsValid,
  takesToken,
  toRequest,
  validate,
  type Draft,
} from './wizard';

function draft(patch: Partial<Draft> = {}): Draft {
  return { ...emptyDraft, ...patch };
}

describe('which fields a source uses', () => {
  it('knows which sources fetch files', () => {
    expect(isRemote('EMPTY')).toBe(false);
    expect(isRemote('GIT_CLONE')).toBe(true);
    expect(isRemote('REMOTE_ARCHIVE')).toBe(true);
  });

  it('offers a branch only where one means something', () => {
    expect(isGitLike('GIT_CLONE')).toBe(true);
    expect(isGitLike('GITHUB_CLI')).toBe(true);
    expect(isGitLike('REMOTE_ARCHIVE')).toBe(false);
  });

  it('does not ask for a token where gh supplies the credential', () => {
    expect(takesToken('GITHUB_CLI')).toBe(false);
    expect(takesToken('GIT_CLONE')).toBe(true);
  });
});

describe('validation', () => {
  it('requires a name', () => {
    expect(validate(draft()).name).toBeDefined();
    expect(validate(draft({ name: 'API', runtime: 'NODEJS' })).name).toBeUndefined();
  });

  it('rejects a name longer than the core accepts', () => {
    expect(validate(draft({ name: 'x'.repeat(61) })).name).toBeDefined();
  });

  it('treats a name of only spaces as missing', () => {
    expect(validate(draft({ name: '   ' })).name).toBeDefined();
  });

  it('makes an empty project choose its own runtime', () => {
    // There are no files to detect from, so the core cannot decide.
    expect(validate(draft({ name: 'API' })).runtime).toBeDefined();
    expect(validate(draft({ name: 'API', runtime: 'NODEJS' })).runtime).toBeUndefined();
  });

  it('lets a remote project leave the runtime to detection', () => {
    const errors = validate(
      draft({ name: 'API', source: 'GIT_CLONE', url: 'https://example.com/a.git' }),
    );
    expect(errors.runtime).toBeUndefined();
  });

  it('insists on https for a clone or an archive', () => {
    expect(
      validate(draft({ name: 'API', source: 'GIT_CLONE', url: 'http://example.com/a.git' })).url,
    ).toBeDefined();
    expect(
      validate(draft({ name: 'API', source: 'GIT_CLONE', url: 'https://example.com/a.git' })).url,
    ).toBeUndefined();
  });

  it('accepts owner/repo or a URL for the GitHub CLI', () => {
    expect(
      validate(draft({ name: 'A', source: 'GITHUB_CLI', url: 'owner/repo' })).url,
    ).toBeUndefined();
    expect(
      validate(draft({ name: 'A', source: 'GITHUB_CLI', url: 'https://github.com/owner/repo' }))
        .url,
    ).toBeUndefined();
    expect(
      validate(draft({ name: 'A', source: 'GITHUB_CLI', url: 'not a repo' })).url,
    ).toBeDefined();
  });

  it('requires an address for every remote source', () => {
    expect(validate(draft({ name: 'A', source: 'REMOTE_ARCHIVE' })).url).toBeDefined();
  });
});

describe('gating the buttons', () => {
  it('allows Create only when everything is answered', () => {
    expect(canCreate(draft())).toBe(false);
    expect(canCreate(draft({ name: 'API', runtime: 'NODEJS' }))).toBe(true);
  });

  it('lets a step advance while a later step is still incomplete', () => {
    // The name is missing, but the source step itself is fine — blocking Next
    // on a field from another step is how a wizard traps someone.
    const partial = draft({ source: 'GIT_CLONE', url: 'https://example.com/a.git' });
    expect(stepIsValid(partial, 'source')).toBe(true);
    expect(stepIsValid(partial, 'details')).toBe(false);
    expect(stepIsValid(partial, 'review')).toBe(false);
  });

  it('points each error at the step that owns it', () => {
    expect(stepForError('url')).toBe('source');
    expect(stepForError('name')).toBe('details');
    expect(stepForError('runtime')).toBe('runtime');
  });
});

describe('building the request', () => {
  it('sends only the fields the chosen source uses', () => {
    const request = toRequest(
      draft({
        name: '  API  ',
        description: ' does things ',
        source: 'REMOTE_ARCHIVE',
        url: ' https://example.com/a.zip ',
        // Left over from an earlier choice of source; must not be sent.
        gitRef: 'main',
        subdirectory: 'packages/api',
        token: 'secret',
      }),
    );

    expect(request.displayName).toBe('API');
    expect(request.description).toBe('does things');
    expect(request.source?.url).toBe('https://example.com/a.zip');
    expect(request.source?.gitRef).toBeUndefined();
    expect(request.source?.subdirectory).toBeUndefined();
    // An archive does take a token.
    expect(request.source?.token).toBe('secret');
  });

  it('never sends a token for the GitHub CLI, which uses gh', () => {
    const request = toRequest(
      draft({ name: 'A', source: 'GITHUB_CLI', url: 'owner/repo', token: 'leftover' }),
    );
    expect(request.source?.token).toBeUndefined();
  });

  it('sends nothing rather than an empty string for an optional field', () => {
    const request = toRequest(
      draft({ name: 'A', source: 'GIT_CLONE', url: 'https://example.com/a.git', gitRef: '   ' }),
    );
    expect(request.source?.gitRef).toBeUndefined();
  });

  it('omits the runtime when detection is wanted', () => {
    const request = toRequest(
      draft({ name: 'A', source: 'GIT_CLONE', url: 'https://example.com/a.git', runtime: null }),
    );
    expect(request.runtime).toBeUndefined();
  });

  it('leaves the url out entirely for an empty project', () => {
    const request = toRequest(draft({ name: 'A', runtime: 'NODEJS', url: 'https://leftover' }));
    expect(request.source?.url).toBeUndefined();
  });
});

describe('progress wording', () => {
  it('says what is actually happening', () => {
    expect(progressLabel('GIT_CLONE')).toMatch(/Cloning/);
    expect(progressLabel('REMOTE_ARCHIVE')).toMatch(/Downloading/);
    expect(progressLabel('EMPTY')).toMatch(/Creating/);
  });
});

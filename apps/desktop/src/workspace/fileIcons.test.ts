import { describe, expect, it } from 'vitest';

import { DEFAULT_FILE_COLOR, fileIconColor, languageName } from './fileIcons';

describe('tinting file icons', () => {
  it('recognises a file by its whole name before its extension', () => {
    // `package.json` and some other `.json` must not look the same.
    expect(fileIconColor('package.json')).not.toBe(fileIconColor('settings.json'));
  });

  it('recognises a name whatever its case', () => {
    expect(fileIconColor('DOCKERFILE')).toBe(fileIconColor('dockerfile'));
  });

  it('falls back to the extension', () => {
    expect(fileIconColor('src/anything.rs'.split('/').pop()!)).toBe(fileIconColor('other.rs'));
  });

  it('gives an unknown extension the neutral tint', () => {
    expect(fileIconColor('mystery.qqq')).toBe(DEFAULT_FILE_COLOR);
  });

  it('treats a dotfile as a name, not an extension', () => {
    // `.gitignore` is not a "gitignore file"; splitting on the dot would tint
    // every dotfile by whatever followed it.
    expect(fileIconColor('.unknownrc')).toBe(DEFAULT_FILE_COLOR);
    expect(fileIconColor('.gitignore')).not.toBe(DEFAULT_FILE_COLOR);
  });
});

describe('naming the language for the status bar', () => {
  it('spells out the ids that have a proper name', () => {
    expect(languageName('typescriptreact')).toBe('TypeScript React');
    expect(languageName('csharp')).toBe('C#');
  });

  it('capitalises an id it does not know rather than showing it raw', () => {
    expect(languageName('nim')).toBe('Nim');
  });

  it('calls a missing language plain text', () => {
    expect(languageName('')).toBe('Plain Text');
  });
});

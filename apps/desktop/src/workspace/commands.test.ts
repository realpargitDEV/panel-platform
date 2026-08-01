import { describe, expect, it } from 'vitest';

import { fuzzyMatch, matchCommands, matchPaths, type Command } from './commands';

function command(title: string, category = 'View'): Command {
  return { id: title, title, category, run: () => {} };
}

describe('fuzzy matching', () => {
  it('matches an initialism spread through the words', () => {
    expect(fuzzyMatch('Toggle Sidebar', 'tgsb')).not.toBeNull();
  });

  it('rejects characters that are not there in order', () => {
    expect(fuzzyMatch('Toggle Sidebar', 'zz')).toBeNull();
    // Right letters, wrong order.
    expect(fuzzyMatch('Toggle Sidebar', 'bs')).toBeNull();
  });

  it('reports where it matched, so the palette can highlight it', () => {
    expect(fuzzyMatch('Save File', 'sf')?.positions).toEqual([0, 5]);
  });

  it('an empty query matches everything with no highlight', () => {
    expect(fuzzyMatch('Anything', '')).toEqual({ positions: [], score: 0 });
  });

  it('is case insensitive in both directions', () => {
    expect(fuzzyMatch('Save File', 'SAVE')).not.toBeNull();
    expect(fuzzyMatch('SAVE FILE', 'save')).not.toBeNull();
  });

  it('scores a run of characters above the same letters scattered', () => {
    const consecutive = fuzzyMatch('Save', 'sav')!.score;
    const scattered = fuzzyMatch('Sxaxv', 'sav')!.score;
    expect(consecutive).toBeGreaterThan(scattered);
  });
});

describe('ranking commands', () => {
  const commands = [
    command('New File', 'File'),
    command('New Folder', 'File'),
    command('Toggle Sidebar'),
    command('Toggle Terminal'),
    command('Open Settings', 'Preferences'),
  ];

  it('returns everything, in the given order, for an empty query', () => {
    expect(matchCommands(commands, '   ').map((match) => match.item.title)).toEqual(
      commands.map((item) => item.title),
    );
  });

  it('puts a title match above a match that needed the category', () => {
    const titles = matchCommands(commands, 'file').map((match) => match.item.title);
    expect(titles[0]).toBe('New File');
    // "New Folder" only matches once "File" the category is included.
    expect(titles).toContain('New Folder');
    expect(titles.indexOf('New File')).toBeLessThan(titles.indexOf('New Folder'));
  });

  it('finds a command by its category alone', () => {
    expect(matchCommands(commands, 'preferences').map((match) => match.item.title)).toEqual([
      'Open Settings',
    ]);
  });

  it('drops commands that do not match at all', () => {
    expect(matchCommands(commands, 'qqq')).toEqual([]);
  });

  it('highlights positions within the title, not the category', () => {
    const [first] = matchCommands(commands, 'new');
    expect(first?.positions).toEqual([0, 1, 2]);
  });
});

describe('ranking paths for quick open', () => {
  const paths = ['src/index.ts', 'src/index/helper.ts', 'docs/readme.md', 'src/app/main.tsx'];

  it('prefers the file whose name was typed over a folder of that name', () => {
    const ranked = matchPaths(paths, 'index').map((match) => match.item);
    expect(ranked[0]).toBe('src/index.ts');
  });

  it('still finds a file by a fragment of its directory', () => {
    expect(matchPaths(paths, 'docs').map((match) => match.item)).toEqual(['docs/readme.md']);
  });

  it('honours the limit so a huge project cannot flood the list', () => {
    const many = Array.from({ length: 500 }, (_, index) => `src/file${index}.ts`);
    expect(matchPaths(many, 'file', 10)).toHaveLength(10);
  });

  it('limits the unfiltered list too', () => {
    const many = Array.from({ length: 500 }, (_, index) => `src/file${index}.ts`);
    expect(matchPaths(many, '', 10)).toHaveLength(10);
  });
});

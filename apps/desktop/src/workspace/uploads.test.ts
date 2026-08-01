import { describe, expect, it } from 'vitest';

import {
  displayDirectory,
  fileNameFromNativePath,
  formatBytes,
  nativeImportLabel,
} from './uploads';

describe('naming a native import', () => {
  it('uses the file name when one thing was dropped', () => {
    expect(nativeImportLabel(['C:\\Users\\me\\notes.txt'], '')).toBe('notes.txt');
  });

  it('shows where a single file is going when it is not the root', () => {
    expect(nativeImportLabel(['/home/me/notes.txt'], 'docs')).toBe('docs/notes.txt');
  });

  it('counts instead of listing when several were dropped', () => {
    expect(nativeImportLabel(['/a/one', '/a/two'], 'src')).toBe('2 items → src');
  });

  it('names the root rather than showing an empty destination', () => {
    expect(nativeImportLabel(['/a/one', '/a/two'], '')).toBe('2 items → the project root');
  });
});

describe('reading a path the operating system gave us', () => {
  it('takes the last component of a Windows path', () => {
    expect(fileNameFromNativePath('C:\\Users\\me\\project')).toBe('project');
  });

  it('takes the last component of a POSIX path', () => {
    expect(fileNameFromNativePath('/home/me/project')).toBe('project');
  });

  it('ignores a trailing separator, which a dropped folder often has', () => {
    expect(fileNameFromNativePath('/home/me/project/')).toBe('project');
  });
});

describe('formatting', () => {
  it('names the root rather than showing nothing', () => {
    expect(displayDirectory('')).toBe('the project root');
    expect(displayDirectory('src/app')).toBe('src/app');
  });

  it('scales bytes to a unit a person reads', () => {
    expect(formatBytes(512)).toBe('512 B');
    expect(formatBytes(2048)).toBe('2.0 KB');
    expect(formatBytes(5 * 1024 * 1024)).toBe('5.0 MB');
    expect(formatBytes(3 * 1024 * 1024 * 1024)).toBe('3.0 GB');
  });
});

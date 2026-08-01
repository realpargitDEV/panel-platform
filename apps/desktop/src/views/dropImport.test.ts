import { describe, expect, it } from 'vitest';

import {
  cleanDropPath,
  directoryPathsForDrop,
  duplicateDroppedFilePath,
  type DroppedFile,
} from './dropImport';

function dropped(relativePath: string): DroppedFile {
  return { relativePath, file: new File(['x'], relativePath) };
}

describe('drop path cleaning', () => {
  it('normalises separators without trimming valid filename characters', () => {
    expect(cleanDropPath(' Project Folder\\space kept .txt')).toBe(
      ' Project Folder/space kept .txt',
    );
  });

  it('keeps every directory needed by nested files and empty folders', () => {
    expect(
      directoryPathsForDrop(
        [dropped('project/src/deep/app.ts'), dropped('project/public/logo.svg')],
        ['project/empty'],
      ),
    ).toEqual(['project', 'project/empty', 'project/public', 'project/src', 'project/src/deep']);
  });

  it('detects duplicate dropped file destinations case-insensitively', () => {
    expect(duplicateDroppedFilePath([dropped('src/App.tsx'), dropped('src/app.tsx')])).toBe(
      'src/app.tsx',
    );
  });
});

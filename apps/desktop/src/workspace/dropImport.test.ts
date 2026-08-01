import { describe, expect, it } from 'vitest';

import {
  cleanDropPath,
  directoryPathsForDrop,
  duplicateDroppedFilePath,
  isInsideDropZone,
  shouldImportBrowserDrop,
  type DroppedFile,
} from './dropImport';

function dropped(relativePath: string): DroppedFile {
  return { relativePath, file: new File(['x'], relativePath) };
}

describe('choosing which drop event imports', () => {
  it('stands down when the OS listener is live, so one drop is imported once', () => {
    expect(shouldImportBrowserDrop(true)).toBe(false);
  });

  it('imports the drop itself when the OS listener could not be registered', () => {
    expect(shouldImportBrowserDrop(false)).toBe(true);
  });

  it('imports a drop exactly once however the two events are interleaved', () => {
    // macOS and Linux deliver both for one gesture, in either order and with
    // any delay between them. Whatever the order, exactly one import happens.
    for (const nativeListenerReady of [true, false]) {
      const imports: string[] = [];
      const browserDrop = () => {
        if (shouldImportBrowserDrop(nativeListenerReady)) imports.push('browser');
      };
      const nativeDrop = () => {
        if (nativeListenerReady) imports.push('native');
      };

      browserDrop();
      nativeDrop();
      expect(imports).toHaveLength(1);
    }
  });
});

describe('native drop targeting', () => {
  // The explorer column, roughly where it sits at the default window size.
  const explorer = { left: 32, top: 180, right: 292, bottom: 760 };

  it('accepts a drop over the explorer', () => {
    expect(isInsideDropZone({ x: 150, y: 400 }, explorer, 1)).toBe(true);
  });

  it('refuses a drop over the editor rather than importing into the project', () => {
    expect(isInsideDropZone({ x: 900, y: 400 }, explorer, 1)).toBe(false);
  });

  it('refuses a drop above the explorer, over the page header', () => {
    expect(isInsideDropZone({ x: 150, y: 40 }, explorer, 1)).toBe(false);
  });

  it('scales physical pixels to CSS pixels on a high-density display', () => {
    // The same on-screen point as the accepted case above, at 200%.
    expect(isInsideDropZone({ x: 300, y: 800 }, explorer, 2)).toBe(true);
    // Physically inside the explorer's pixels, but that is the editor in CSS
    // pixels — the unscaled comparison would wrongly accept this.
    expect(isInsideDropZone({ x: 1800, y: 800 }, explorer, 2)).toBe(false);
  });

  it('treats a missing device pixel ratio as 1 instead of dividing by zero', () => {
    expect(isInsideDropZone({ x: 150, y: 400 }, explorer, 0)).toBe(true);
  });

  it('excludes the far edges so adjacent panels never both claim a drop', () => {
    expect(isInsideDropZone({ x: 32, y: 180 }, explorer, 1)).toBe(true);
    expect(isInsideDropZone({ x: 292, y: 400 }, explorer, 1)).toBe(false);
    expect(isInsideDropZone({ x: 150, y: 760 }, explorer, 1)).toBe(false);
  });
});

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

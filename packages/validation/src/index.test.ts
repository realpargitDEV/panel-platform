import { describe, expect, it } from 'vitest';

import {
  buildEnvExample,
  isValidEnvKey,
  parseEnvFile,
  validateHostPort,
  validateProjectName,
  validateResources,
} from './index.js';

describe('isValidEnvKey', () => {
  it('accepts conventional names', () => {
    expect(isValidEnvKey('DISCORD_TOKEN')).toBe(true);
    expect(isValidEnvKey('_private')).toBe(true);
    expect(isValidEnvKey('PORT2')).toBe(true);
  });

  it('rejects names that could confuse a shell or an env file', () => {
    for (const key of ['2FA', 'MY-KEY', 'MY KEY', 'KEY=X', '', 'KEY;rm -rf /', 'KEY\nOTHER']) {
      expect(isValidEnvKey(key), key).toBe(false);
    }
  });
});

describe('validateProjectName', () => {
  it('accepts an ordinary name', () => {
    expect(validateProjectName('My Discord Bot')).toEqual([]);
  });

  it('requires something non-blank', () => {
    expect(validateProjectName('   ')).toHaveLength(1);
    expect(validateProjectName('')).toHaveLength(1);
  });

  it('rejects control characters, which render as invisible damage', () => {
    const issues = validateProjectName('bot\u0007name');
    expect(issues).toHaveLength(1);
    expect(issues[0]?.message).toMatch(/control characters/i);
  });

  it('allows characters that would be unsafe in a path, because it is display only', () => {
    // The display name never becomes a directory or a container name, so these
    // are fine here. `docs/security.md` §5 is the reasoning.
    expect(validateProjectName('../../etc/passwd')).toEqual([]);
    expect(validateProjectName('C:\\Windows')).toEqual([]);
  });

  it('caps the length', () => {
    expect(validateProjectName('a'.repeat(65))).toHaveLength(1);
    expect(validateProjectName('a'.repeat(64))).toEqual([]);
  });
});

describe('validateResources', () => {
  const valid = {
    memory_limit_mb: 512,
    cpu_limit_cores: 1,
    storage_limit_mb: 2048,
    process_limit: 128,
  };

  it('accepts the defaults', () => {
    expect(validateResources(valid)).toEqual([]);
  });

  it('rejects each field outside its bounds', () => {
    expect(validateResources({ ...valid, memory_limit_mb: 32 })).toHaveLength(1);
    expect(validateResources({ ...valid, cpu_limit_cores: 0 })).toHaveLength(1);
    expect(validateResources({ ...valid, process_limit: 4 })).toHaveLength(1);
    expect(validateResources({ ...valid, memory_limit_mb: Number.NaN })).toHaveLength(1);
  });

  it('reports every offending field rather than only the first', () => {
    const issues = validateResources({
      memory_limit_mb: 1,
      cpu_limit_cores: 999,
      storage_limit_mb: 1,
      process_limit: 1,
    });
    expect(issues).toHaveLength(4);
  });
});

describe('validateHostPort', () => {
  it('accepts unprivileged ports', () => {
    expect(validateHostPort(8080)).toEqual([]);
    expect(validateHostPort(1024)).toEqual([]);
    expect(validateHostPort(65535)).toEqual([]);
  });

  it('refuses privileged ports instead of quietly raising them', () => {
    expect(validateHostPort(80)).toHaveLength(1);
    expect(validateHostPort(443)).toHaveLength(1);
    expect(validateHostPort(1023)).toHaveLength(1);
  });

  it('refuses non-integers and out-of-range values', () => {
    expect(validateHostPort(8080.5)).toHaveLength(1);
    expect(validateHostPort(70000)).toHaveLength(1);
    expect(validateHostPort(-1)).toHaveLength(1);
  });
});

describe('parseEnvFile', () => {
  it('parses ordinary lines and strips quotes', () => {
    const result = parseEnvFile('A=1\nB="two"\nC=\'three\'\n');
    expect(result.entries).toEqual([
      { key: 'A', value: '1' },
      { key: 'B', value: 'two' },
      { key: 'C', value: 'three' },
    ]);
  });

  it('ignores comments, blank lines and export prefixes', () => {
    const result = parseEnvFile('# comment\n\nexport TOKEN=abc\n');
    expect(result.entries).toEqual([{ key: 'TOKEN', value: 'abc' }]);
  });

  it('keeps the first of a duplicated key and reports it', () => {
    const result = parseEnvFile('A=first\nA=second\n');
    expect(result.entries).toEqual([{ key: 'A', value: 'first' }]);
    expect(result.duplicates).toEqual(['A']);
  });

  it('reports invalid keys rather than importing them', () => {
    const result = parseEnvFile('GOOD=1\nBAD-KEY=2\n');
    expect(result.entries).toEqual([{ key: 'GOOD', value: '1' }]);
    expect(result.invalidKeys).toEqual(['BAD-KEY']);
  });

  it('counts lines it could not read', () => {
    const result = parseEnvFile('this is not an assignment\n=novalue\n');
    expect(result.entries).toEqual([]);
    expect(result.skippedLines).toBe(2);
  });

  it('preserves an equals sign inside a value', () => {
    const result = parseEnvFile('URL=postgres://u:p@h/db?a=b\n');
    expect(result.entries[0]?.value).toBe('postgres://u:p@h/db?a=b');
  });

  it('handles CRLF files', () => {
    const result = parseEnvFile('A=1\r\nB=2\r\n');
    expect(result.entries).toHaveLength(2);
    expect(result.entries[1]).toEqual({ key: 'B', value: '2' });
  });
});

describe('buildEnvExample', () => {
  it('emits keys with empty values, sorted and deduplicated', () => {
    const output = buildEnvExample(['ZED', 'ALPHA', 'ZED']);
    expect(output).toContain('ALPHA=\nZED=');
    expect(output.startsWith('#')).toBe(true);
  });

  it('never emits a value, which is the entire point of the export', () => {
    const output = buildEnvExample(['DISCORD_TOKEN']);
    expect(output).toContain('DISCORD_TOKEN=');
    expect(output).not.toMatch(/DISCORD_TOKEN=.+/);
  });

  it('drops keys that would not be valid', () => {
    expect(buildEnvExample(['BAD-KEY'])).not.toContain('BAD-KEY');
  });
});

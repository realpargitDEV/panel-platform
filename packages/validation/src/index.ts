/**
 * Validation that exists purely for form ergonomics — telling someone their
 * environment variable name is invalid as they type it, rather than after a
 * round trip.
 *
 * These rules mirror constraints the agent enforces authoritatively (SQLite
 * `CHECK` constraints and Rust validators). Passing here is never treated as
 * permission to skip the server-side check; see `docs/security.md` §5.
 */

export const ENV_KEY_PATTERN = /^[A-Za-z_][A-Za-z0-9_]*$/;

/** Matches the `CHECK (key GLOB ...)` constraint on `environment_variables`. */
export function isValidEnvKey(key: string): boolean {
  return ENV_KEY_PATTERN.test(key);
}

export interface FieldIssue {
  field: string;
  message: string;
}

export const PROJECT_NAME_MAX = 64;
export const PROJECT_DESCRIPTION_MAX = 500;

/**
 * The display name is presentation only — it never becomes a path, a slug or a
 * container name, so this checks readability rather than safety. Control
 * characters are rejected because they render as invisible damage in a list.
 */
export function validateProjectName(name: string): FieldIssue[] {
  const issues: FieldIssue[] = [];
  const trimmed = name.trim();

  if (trimmed.length === 0) {
    issues.push({ field: 'display_name', message: 'Give the project a name.' });
    return issues;
  }
  if (trimmed.length > PROJECT_NAME_MAX) {
    issues.push({
      field: 'display_name',
      message: `Keep the name under ${PROJECT_NAME_MAX} characters.`,
    });
  }
  // eslint-disable-next-line no-control-regex
  if (/[\u0000-\u001F\u007F]/.test(name)) {
    issues.push({ field: 'display_name', message: 'Remove control characters from the name.' });
  }
  return issues;
}

export const RESOURCE_BOUNDS = {
  memoryMb: { min: 64, max: 65536 },
  cpuCores: { min: 0.1, max: 64 },
  storageMb: { min: 128, max: 1048576 },
  processes: { min: 8, max: 4096 },
} as const;

export interface ResourceInput {
  memory_limit_mb: number;
  cpu_limit_cores: number;
  storage_limit_mb: number;
  process_limit: number;
}

/** Mirrors the `CHECK` constraints on the `projects` table. */
export function validateResources(input: ResourceInput): FieldIssue[] {
  const issues: FieldIssue[] = [];
  const check = (
    value: number,
    field: keyof ResourceInput,
    bounds: { min: number; max: number },
    unit: string,
  ) => {
    if (!Number.isFinite(value) || value < bounds.min || value > bounds.max) {
      issues.push({
        field,
        message: `Choose between ${bounds.min} and ${bounds.max} ${unit}.`,
      });
    }
  };

  check(input.memory_limit_mb, 'memory_limit_mb', RESOURCE_BOUNDS.memoryMb, 'MB');
  check(input.cpu_limit_cores, 'cpu_limit_cores', RESOURCE_BOUNDS.cpuCores, 'cores');
  check(input.storage_limit_mb, 'storage_limit_mb', RESOURCE_BOUNDS.storageMb, 'MB');
  check(input.process_limit, 'process_limit', RESOURCE_BOUNDS.processes, 'processes');
  return issues;
}

export const PORT_BOUNDS = { min: 1024, max: 65535 } as const;

/**
 * Ports below 1024 are refused rather than clamped. A user asking to publish on
 * 80 has a different intent than one who would accept 1024, and silently
 * changing it would surprise them.
 */
export function validateHostPort(port: number): FieldIssue[] {
  if (!Number.isInteger(port) || port < PORT_BOUNDS.min || port > PORT_BOUNDS.max) {
    return [
      {
        field: 'host_port',
        message: `Choose a port between ${PORT_BOUNDS.min} and ${PORT_BOUNDS.max}. Privileged ports are not available.`,
      },
    ];
  }
  return [];
}

export interface ParsedEnvFile {
  entries: Array<{ key: string; value: string }>;
  duplicates: string[];
  invalidKeys: string[];
  skippedLines: number;
}

/**
 * Parse a pasted `.env`. Tolerant of real-world files — comments, blank lines,
 * `export` prefixes, quoted values — but never invents a key it cannot validate.
 */
export function parseEnvFile(contents: string): ParsedEnvFile {
  const entries: Array<{ key: string; value: string }> = [];
  const seen = new Set<string>();
  const duplicates: string[] = [];
  const invalidKeys: string[] = [];
  let skippedLines = 0;

  for (const rawLine of contents.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (line.length === 0 || line.startsWith('#')) continue;

    const withoutExport = line.startsWith('export ') ? line.slice('export '.length).trim() : line;
    const separator = withoutExport.indexOf('=');
    if (separator <= 0) {
      skippedLines += 1;
      continue;
    }

    const key = withoutExport.slice(0, separator).trim();
    let value = withoutExport.slice(separator + 1).trim();

    if (
      (value.startsWith('"') && value.endsWith('"') && value.length >= 2) ||
      (value.startsWith("'") && value.endsWith("'") && value.length >= 2)
    ) {
      value = value.slice(1, -1);
    }

    if (!isValidEnvKey(key)) {
      invalidKeys.push(key);
      continue;
    }
    if (seen.has(key)) {
      duplicates.push(key);
      continue;
    }
    seen.add(key);
    entries.push({ key, value });
  }

  return { entries, duplicates, invalidKeys, skippedLines };
}

/**
 * Build a `.env.example` from known keys. Values are always empty — the whole
 * point of the export is that it is safe to commit.
 */
export function buildEnvExample(keys: readonly string[]): string {
  const unique = [...new Set(keys)].filter(isValidEnvKey).sort();
  const header = '# Generated by Project Host. Values are intentionally blank.\n';
  return header + unique.map((key) => `${key}=`).join('\n') + (unique.length > 0 ? '\n' : '');
}

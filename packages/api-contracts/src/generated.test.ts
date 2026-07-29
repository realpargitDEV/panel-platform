import { describe, expect, it } from 'vitest';

import {
  apiErrorSchema,
  createProjectRequestSchema,
  envVarSummarySchema,
  projectDetailSchema,
  projectStatusSchema,
} from './generated.js';

/**
 * These test the generated artefact, not hand-written logic. They exist because
 * the generator is code too: a change to `crates/api-types/src/codegen.rs` that
 * silently produced a permissive schema would otherwise go unnoticed until a
 * malformed payload reached the UI.
 */
describe('generated Zod schemas', () => {
  it('accepts the enum values the agent actually sends', () => {
    expect(projectStatusSchema.parse('RUNNING')).toBe('RUNNING');
    expect(projectStatusSchema.parse('UNHEALTHY')).toBe('UNHEALTHY');
    expect(projectStatusSchema.parse('ARCHIVED')).toBe('ARCHIVED');
  });

  it('rejects an enum value belonging to a different enum', () => {
    // OOM_KILLED is a ContainerEventType, not a ProjectStatus.
    expect(projectStatusSchema.safeParse('OOM_KILLED').success).toBe(false);
    expect(projectStatusSchema.safeParse('running').success).toBe(false);
  });

  it('models a secret variable as no value plus is_set', () => {
    const secret = {
      id: 'env_0193000000007000a000000000000001',
      key: 'DISCORD_TOKEN',
      is_secret: true,
      is_set: true,
      restart_required: true,
      updated_at: '2026-07-29T00:00:00Z',
    };
    const parsed = envVarSummarySchema.parse(secret);
    expect(parsed.value).toBeUndefined();
    expect(parsed.is_set).toBe(true);
  });

  it('requires the fields the agent guarantees', () => {
    const result = envVarSummarySchema.safeParse({ key: 'A' });
    expect(result.success).toBe(false);
  });

  it('validates an error envelope', () => {
    const parsed = apiErrorSchema.parse({
      code: 'PROJECT_LOCKED',
      message: 'This project is being restored.',
      request_id: 'req_1',
    });
    expect(parsed.code).toBe('PROJECT_LOCKED');
  });

  it('rejects an unknown error code, so a typo cannot be mistaken for a real one', () => {
    const result = apiErrorSchema.safeParse({
      code: 'SOMETHING_ELSE',
      message: 'x',
      request_id: 'r',
    });
    expect(result.success).toBe(false);
  });

  it('exposes ProjectDetail as one flat object, matching the wire format', () => {
    const shape = Object.keys(projectDetailSchema.shape);
    // Fields inherited through #[serde(flatten)] must be present at the top level.
    expect(shape).toContain('display_name');
    expect(shape).toContain('status');
    // And the flattened struct must not appear as a nested key.
    expect(shape).not.toContain('summary');
  });

  it('requires a source and a runtime when creating a project', () => {
    const result = createProjectRequestSchema.safeParse({ display_name: 'Bot' });
    expect(result.success).toBe(false);
  });
});

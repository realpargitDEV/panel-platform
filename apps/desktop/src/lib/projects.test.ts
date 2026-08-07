import { describe, expect, it } from 'vitest';

import type { ProjectSummary } from '../api';
import {
  actionTone,
  attentionReason,
  countByStatus,
  describeAction,
  healthLook,
  isFailed,
  needsAttention,
  runControls,
  statusLook,
} from './projects';

function project(status: string, desiredState = 'RUNNING'): ProjectSummary {
  return {
    id: `id-${status}-${desiredState}`,
    slug: 'demo',
    displayName: 'Demo',
    description: '',
    projectType: 'NODEJS',
    status,
    desiredState,
    color: null,
    runMode: 'DOCKER',
  };
}

describe('reading a status', () => {
  it('colours a running project green and a failed one red', () => {
    expect(statusLook('RUNNING').tone).toBe('ok');
    expect(statusLook('BUILD_FAILED').tone).toBe('danger');
  });

  it('leaves a stopped project neutral', () => {
    // Stopping a project on purpose is normal. Colouring it as a fault is how
    // people learn to ignore red.
    expect(statusLook('STOPPED').tone).toBe('neutral');
  });

  it('marks the in-between states as transitioning, so controls disable', () => {
    for (const status of ['STARTING', 'STOPPING', 'RESTARTING', 'BUILDING']) {
      expect(statusLook(status).transitioning).toBe(true);
    }
    expect(statusLook('RUNNING').transitioning).toBe(false);
  });

  it('renders a status it has never seen rather than showing nothing', () => {
    expect(statusLook('SOME_NEW_STATE').label).toBe('Some new state');
  });

  it('does not care about case', () => {
    expect(statusLook('running').label).toBe('Running');
  });
});

describe('reading health', () => {
  it('separates healthy from having no check at all', () => {
    expect(healthLook('HEALTHY').tone).toBe('ok');
    expect(healthLook('UNHEALTHY').tone).toBe('danger');
    expect(healthLook('NONE').label).toBe('No health check');
  });
});

describe('which projects need attention', () => {
  it('flags anything that failed', () => {
    expect(needsAttention(project('BUILD_FAILED'))).toBe(true);
    expect(isFailed('CRASHED')).toBe(true);
  });

  it('flags a project that should be running and is not', () => {
    expect(needsAttention(project('STOPPED', 'RUNNING'))).toBe(true);
    expect(attentionReason(project('STOPPED', 'RUNNING'))).toBe('Should be running');
  });

  it('leaves a project alone when it was asked to stop', () => {
    expect(needsAttention(project('STOPPED', 'STOPPED'))).toBe(false);
  });

  it('does not flag one that is still on its way up', () => {
    // A starting project is not a problem; it is a project starting.
    expect(needsAttention(project('STARTING', 'RUNNING'))).toBe(false);
  });

  it('leaves a healthy running project alone', () => {
    expect(needsAttention(project('RUNNING', 'RUNNING'))).toBe(false);
  });
});

describe('counting', () => {
  it('splits the list by state', () => {
    const counts = countByStatus([
      project('RUNNING', 'RUNNING'),
      project('RUNNING', 'RUNNING'),
      project('STOPPED', 'STOPPED'),
      project('FAILED', 'RUNNING'),
    ]);

    expect(counts).toEqual({ total: 4, running: 2, stopped: 1, failed: 1, attention: 1 });
  });

  it('counts an empty list without dividing by anything', () => {
    expect(countByStatus([])).toEqual({
      total: 0,
      running: 0,
      stopped: 0,
      failed: 0,
      attention: 0,
    });
  });
});

describe('describing an audit entry', () => {
  it('turns a dotted action into a phrase', () => {
    expect(describeAction('project.start', null)).toBe('Started project');
  });

  it('appends the target when the log recorded one', () => {
    expect(describeAction('project.start', 'demo-api')).toBe('Started project demo-api');
  });

  it('renders an action it does not know rather than the raw string', () => {
    expect(describeAction('backup.prune', null)).toBe('Backup prune');
  });

  it('colours the result', () => {
    expect(actionTone('SUCCESS')).toBe('ok');
    expect(actionTone('FAILURE')).toBe('danger');
    expect(actionTone('DENIED')).toBe('warn');
    expect(actionTone('whatever')).toBe('neutral');
  });
});

describe('runControls', () => {
  function withMode(runMode: string, status = 'STOPPED'): ProjectSummary {
    return { ...project(status), runMode };
  }

  it('blocks a Docker project when there is no daemon', () => {
    const { blocked, reason } = runControls(withMode('DOCKER'), {
      busy: false,
      dockerAvailable: false,
    });
    expect(blocked).toBe(true);
    expect(reason).toBe('Docker is not available');
  });

  // The whole point of host mode. Before this, a machine with no daemon had
  // every control greyed out and the feature was unreachable.
  it('leaves a host project usable when there is no daemon', () => {
    expect(runControls(withMode('HOST'), { busy: false, dockerAvailable: false })).toEqual({
      blocked: false,
    });
  });

  it('blocks either mode while the project is transitioning', () => {
    for (const mode of ['DOCKER', 'HOST']) {
      const { blocked, reason } = runControls(withMode(mode, 'STARTING'), {
        busy: false,
        dockerAvailable: true,
      });
      expect(blocked).toBe(true);
      expect(reason).toBe('The project is starting');
    }
  });

  it('blocks either mode while another action is running', () => {
    for (const mode of ['DOCKER', 'HOST']) {
      expect(runControls(withMode(mode), { busy: true, dockerAvailable: true }).blocked).toBe(true);
    }
  });

  it('allows a Docker project when the daemon is there', () => {
    expect(runControls(withMode('DOCKER'), { busy: false, dockerAvailable: true })).toEqual({
      blocked: false,
    });
  });
});

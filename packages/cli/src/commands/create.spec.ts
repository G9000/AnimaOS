import { afterEach, beforeEach, describe, expect, it } from 'vitest';
import { REMOTE_PROVIDER_IDS, providerKeyEnvNames } from '../provider-config.js';
import {
  buildSingleAgentConfig,
  getCreateProviderIssue,
  resolveCreateProvider,
  resolveCreateTarget,
} from './create.js';

describe('create command flow helpers', () => {
  const envSnapshot = new Map<string, string | undefined>();

  beforeEach(() => {
    envSnapshot.clear();
    for (const provider of REMOTE_PROVIDER_IDS) {
      for (const envName of providerKeyEnvNames(provider)) {
        if (!envSnapshot.has(envName)) {
          envSnapshot.set(envName, process.env[envName]);
        }
        delete process.env[envName];
      }
    }
  });

  afterEach(() => {
    for (const [envName, value] of envSnapshot) {
      if (value === undefined) {
        delete process.env[envName];
      } else {
        process.env[envName] = value;
      }
    }
  });

  it('routes the explicit agent subflow', () => {
    expect(resolveCreateTarget('agent', 'helper', undefined)).toEqual({
      kind: 'agent',
      nameArg: 'helper',
    });
    expect(resolveCreateTarget('helper', undefined, 'agent')).toEqual({
      kind: 'agent',
      nameArg: 'helper',
    });
    expect(resolveCreateTarget(undefined, undefined, undefined)).toEqual({
      kind: 'agent',
    });
  });

  it('points removed agency creation and unknown kinds at the agent flow', () => {
    expect(resolveCreateTarget('agency', 'lab', undefined).error).toContain(
      'Agency generation was removed'
    );
    expect(resolveCreateTarget('content-team', undefined, undefined).error).toContain(
      'Unknown create kind'
    );
    expect(resolveCreateTarget('agent', 'x', 'nope').error).toContain('Unknown create kind');
  });

  it('falls back to deterministic scaffolding when no provider is configured', () => {
    expect(resolveCreateProvider(undefined)).toBe('deterministic');
  });

  it('auto-selects configured remote providers before deterministic scaffolding', () => {
    process.env.OPENAI_API_KEY = 'test-key';

    expect(resolveCreateProvider(undefined)).toBe('openai');
  });

  it('reports explicit unknown providers before falling back to auto mode', () => {
    expect(getCreateProviderIssue('not-a-provider')).toContain('Unknown provider');
    expect(getCreateProviderIssue(undefined)).toBeUndefined();
    expect(getCreateProviderIssue('deterministic')).toBeUndefined();
  });

  it('builds single-agent workspaces as a launchable anima.yaml shape', () => {
    const config = buildSingleAgentConfig({
      name: 'medicine-helper',
      description: 'testing a medicine',
      provider: 'deterministic',
      model: 'local-model',
    });

    expect(config).toMatchObject({
      name: 'medicine-helper',
      provider: 'deterministic',
      model: 'local-model',
      strategy: 'supervisor',
      agents: [],
      orchestrator: {
        name: 'medicine-helper',
        role: 'orchestrator',
        model: 'local-model',
      },
    });
    expect(config.orchestrator.system).toContain('testing a medicine');
  });
});
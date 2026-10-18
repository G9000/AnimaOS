import { afterEach, describe, expect, it, vi } from 'vitest';
import type { AgentDetail } from './types';
import {
  readActiveAgent,
  saveActiveAgent,
  selectActiveAgent,
} from './active-agent';

function agent(
  id: string,
  createdAt: number,
  overrides: Partial<AgentDetail> = {},
): AgentDetail {
  return {
    id,
    name: id,
    provider: 'default',
    model: 'test',
    toolNames: [],
    created_at_ms: createdAt,
    status: 'Idle',
    token_usage: { prompt_tokens: 0, completion_tokens: 0, total_tokens: 0 },
    messages: [],
    ...overrides,
  } as AgentDetail;
}

afterEach(() => {
  localStorage.clear();
  vi.restoreAllMocks();
});

describe('active agent', () => {
  const nova = agent('nova', 1);
  const scout = agent('scout', 2);
  const helper = agent('helper', 3, { workspaceRole: 'helper' });

  it('uses the picked companion and falls back when it is gone or a helper', () => {
    expect(selectActiveAgent([nova, scout], 'scout')).toBe(scout);
    expect(selectActiveAgent([nova], 'scout')).toBe(nova);
    expect(selectActiveAgent([nova, helper], 'helper')).toBe(nova);
    expect(selectActiveAgent([nova, scout], null)).toBe(nova);
  });

  it('remembers the pick and survives storage that refuses it', () => {
    saveActiveAgent('scout');
    expect(readActiveAgent()).toBe('scout');
    vi.spyOn(Storage.prototype, 'getItem').mockImplementation(() => {
      throw new Error('blocked');
    });
    vi.spyOn(Storage.prototype, 'setItem').mockImplementation(() => {
      throw new Error('blocked');
    });
    expect(() => saveActiveAgent('nova')).not.toThrow();
    expect(readActiveAgent()).toBeNull();
  });
});

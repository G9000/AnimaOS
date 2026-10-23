import { describe, expect, it } from 'vitest';

import {
  createDaemonClient,
  type AgentSnapshot,
  type AgentUpdateInput,
  type DelegationPolicy,
} from '../src/index.js';

describe('owner delegation policy contract', () => {
  it('sends typed create and PATCH settings and retains the returned policy', async () => {
    const delegation: DelegationPolicy = {
      maxConcurrent: 2,
      maxStartsPerRun: 3,
      maxDepth: 1,
      allowContextFork: false,
      maxToolIterations: 6,
      timeoutMs: 30000,
      maxOutputTokens: 2048,
      routes: [
        { id: 'review', provider: 'openai', model: 'model-a', fallbacks: [] },
      ],
    };
    const snapshot: AgentSnapshot = {
      state: {
        id: 'agent-1',
        name: 'Researcher',
        status: 'idle',
        createdAtMs: 1,
        tokenUsage: { promptTokens: 0, completionTokens: 0, totalTokens: 0 },
        config: {
          name: 'Researcher',
          model: 'model-a',
          provider: 'openai',
          bio: null,
          lore: null,
          knowledge: null,
          topics: null,
          adjectives: null,
          style: null,
          system: null,
          tools: [],
          plugins: null,
          settings: {
            temperature: 0.2,
            maxTokens: null,
            timeoutMs: null,
            maxRetries: null,
            maxToolIterations: null,
            additional: { customFlag: true },
            delegation,
          },
        },
      },
      messages: [],
      messageCount: 0,
      eventCount: 0,
      lastTask: null,
    };
    const requests: { url: string; init?: RequestInit }[] = [];
    const client = createDaemonClient({
      baseUrl: 'http://daemon.test',
      fetch: async (input, init) => {
        requests.push({ url: String(input), init });
        return new Response(JSON.stringify({ agent: snapshot }), {
          headers: { 'content-type': 'application/json' },
        });
      },
    });

    const created = await client.agents.create({
      name: 'Researcher',
      model: 'model-a',
      settings: { delegation, temperature: 0.2, customFlag: true },
    });
    expect(created.state.config.settings?.delegation).toEqual(delegation);
    expect(
      JSON.parse(String(requests[0].init?.body)).settings.delegation,
    ).toEqual(delegation);

    const patch: AgentUpdateInput = {
      settings: {
        delegation,
        executionLimits: { maxModelAttempts: 1, maxRetries: 0 },
      },
    };
    const updated = await client.agents.update('agent-1', patch);
    expect(requests[1].url).toBe('http://daemon.test/api/agents/agent-1');
    expect(requests[1].init?.method).toBe('PATCH');
    expect(JSON.parse(String(requests[1].init?.body))).toEqual(patch);
    expect(updated.state.config.settings?.delegation?.routes?.[0].id).toBe(
      'review',
    );
    expect(updated.state.config.settings?.additional.customFlag).toBe(true);
  });
});

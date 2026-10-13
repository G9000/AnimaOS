import { describe, expect, it } from 'vitest';

import { createDaemonClient } from './index.js';

function transport(respond: (url: string, init?: RequestInit) => Response) {
  const requests: { url: string; init?: RequestInit }[] = [];
  const client = createDaemonClient({
    baseUrl: '',
    fetch: async (url, init) => {
      requests.push({ url: String(url), init });
      return respond(String(url), init);
    },
  });
  return { learning: client.learning, requests };
}

const review = {
  id: 'review-1',
  agentId: 'agent/a',
  sourceRunId: 'run:1',
  sessionId: 'chat:1',
  trigger: 'manual',
  policyVersion: 1,
  evidenceDigest: 'digest',
  evidenceBrief: 'Task completed.',
  evidenceAnchors: ['run:1'],
  state: 'completed',
  reservedAtMs: 1,
  startedAtMs: 2,
  finishedAtMs: 3,
  model: 'gpt-5.5',
  provider: 'openai',
  usage: { promptTokens: 10, completionTokens: 5, totalTokens: 15 },
  usageComplete: true,
  error: null,
  proposals: [
    {
      id: 'proposal-1',
      observation: 'A task pattern.',
      suggestion: 'Keep the verified preference.',
      anchors: ['run:1'],
      decision: 'pending',
      decisionVersion: 0,
    },
  ],
};

describe('learning client', () => {
  it('uses owner policy, review, cancellation, and versioned proposal routes', async () => {
    const { learning, requests } = transport((url) => {
      if (url.endsWith('/learning-policy'))
        return Response.json({
          autoEnabled: false,
          dailyLimit: 2,
          pendingLimit: 10,
          timeoutMs: 20_000,
        });
      if (url.endsWith('/learning-reviews'))
        return Response.json({ reviews: [review] });
      return Response.json({ review });
    });

    expect(await learning.policy('agent/a')).toMatchObject({
      autoEnabled: false,
      dailyLimit: 2,
    });
    await learning.setPolicy('agent/a', { autoEnabled: true });
    expect(await learning.requestReview('agent/a', 'run:1')).toEqual(review);
    expect(await learning.listReviews('agent/a')).toEqual([review]);
    await learning.getReview('agent/a', 'review/1');
    await learning.cancelReview('agent/a', 'review/1');
    await learning.decideProposal('agent/a', 'review/1', 'proposal/1', {
      decision: 'acknowledged',
      expectedVersion: 0,
    });

    expect(
      requests.map(({ url, init }) => [init?.method ?? 'GET', url, init?.body]),
    ).toEqual([
      ['GET', '/api/agents/agent%2Fa/learning-policy', undefined],
      [
        'PATCH',
        '/api/agents/agent%2Fa/learning-policy',
        JSON.stringify({ autoEnabled: true }),
      ],
      ['POST', '/api/agents/agent%2Fa/runs/run%3A1/learning-review', '{}'],
      ['GET', '/api/agents/agent%2Fa/learning-reviews', undefined],
      ['GET', '/api/agents/agent%2Fa/learning-reviews/review%2F1', undefined],
      [
        'POST',
        '/api/agents/agent%2Fa/learning-reviews/review%2F1/cancel',
        '{}',
      ],
      [
        'PATCH',
        '/api/agents/agent%2Fa/learning-reviews/review%2F1/proposals/proposal%2F1',
        JSON.stringify({ decision: 'acknowledged', expectedVersion: 0 }),
      ],
    ]);
  });
});

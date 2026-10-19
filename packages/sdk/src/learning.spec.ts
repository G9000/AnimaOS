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
  it('creates a pending skill draft with exact proposal/source CAS and bytes', async () => {
    const draft = {
      id: 'skd-learning-1',
      slug: 'source-checking',
      name: 'Source checking',
      description: 'Check cited sources.',
      body: 'Verify the cited source before summarizing.',
      source: 'agent',
      proposedBy: { agentId: 'agent/a', sessionId: 'chat:1', runId: 'run:1' },
      baseHash: null,
      currentHash: null,
      stale: false,
      fileHash: null,
      createdAtMs: 4,
      status: 'pending',
      decidedAtMs: null,
      problem: null,
      learningSource: {
        agentId: 'agent/a',
        reviewId: 'review/1',
        proposalId: 'proposal/1',
        sourceRunId: 'run:1',
        sessionId: 'chat:1',
        evidenceDigest: 'digest-current',
        decisionVersion: 3,
      },
    };
    let attempts = 0;
    const { learning, requests } = transport((url) => {
      if (!url.endsWith('/skill-draft')) return Response.json({ review });
      attempts += 1;
      return Response.json({ draft }, { status: attempts === 1 ? 201 : 200 });
    });
    const input = {
      expectedVersion: 3,
      evidenceDigest: 'digest-current',
      skill: {
        slug: 'source-checking',
        name: 'Source checking',
        description: 'Check cited sources.',
        body: 'Verify the cited source before summarizing.',
        expectedHash: null,
      },
    };

    expect(
      await learning.createSkillDraft(
        'agent/a',
        'review/1',
        'proposal/1',
        input,
      ),
    ).toEqual(draft);
    expect(
      await learning.createSkillDraft(
        'agent/a',
        'review/1',
        'proposal/1',
        input,
      ),
    ).toEqual(draft);
    expect(requests).toHaveLength(2);
    expect(requests[0]?.url).toBe(
      '/api/agents/agent%2Fa/learning-reviews/review%2F1/proposals/proposal%2F1/skill-draft',
    );
    expect(requests[0]?.init?.method).toBe('POST');
    expect(requests[0]?.init?.body).toBe(JSON.stringify(input));
    expect(requests[1]?.url).toBe(requests[0]?.url);
    expect(requests[1]?.init?.body).toBe(requests[0]?.init?.body);
  });

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

  it('reads conversation reviews with core-memory edits and undoes an applied edit', async () => {
    const conversation = {
      ...review,
      trigger: 'conversation' as const,
      evidenceAnchors: ['turn:run-1'],
      proposals: [
        {
          id: 'edit-1',
          observation: 'The owner asked for kilometres.',
          suggestion: 'Add to About you: Prefers metric units',
          anchors: ['turn:run-1'],
          decision: 'acknowledged' as const,
          decisionVersion: 1,
          memoryId: 'mem-1',
          coreEdit: {
            action: 'add' as const,
            block: 'user' as const,
            text: 'Prefers metric units',
          },
        },
        {
          id: 'edit-2',
          observation: 'Over budget.',
          suggestion: 'Add to Agent notes: x',
          anchors: ['turn:run-1'],
          decision: 'failed' as const,
          decisionVersion: 1,
          coreEdit: {
            action: 'add' as const,
            block: 'notes' as const,
            text: 'x',
            error:
              'Agent notes is full (2,500/2,500 chars). Replace or remove an entry first.',
          },
        },
      ],
    };
    const { learning, requests } = transport((url) =>
      url.endsWith('/learning-reviews')
        ? Response.json({ reviews: [conversation] })
        : Response.json({ review: conversation }),
    );

    const [listed] = await learning.listReviews('agent/a');
    expect(listed?.trigger).toBe('conversation');
    expect(listed?.proposals[0]?.coreEdit?.text).toBe('Prefers metric units');
    expect(listed?.proposals[1]?.decision).toBe('failed');
    await learning.undoLesson('agent/a', 'review/1', 'edit-1', {
      expectedVersion: 1,
    });
    expect(requests[1]).toMatchObject({
      url: '/api/agents/agent%2Fa/learning-reviews/review%2F1/proposals/edit-1/undo',
    });
    expect(requests[1]?.init?.body).toBe(
      JSON.stringify({ expectedVersion: 1 }),
    );
  });
});

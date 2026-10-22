import type { Approval, ClarificationRequest } from '@animaos/sdk';

export function approval(overrides: Partial<Approval> = {}): Approval {
  return {
    id: 'approval-a',
    agentId: 'agent-a',
    sessionId: 'session-a',
    runId: 'run-a',
    toolCallId: 'tool-a',
    tool: 'shell',
    class: 'exec',
    arguments: '{"command":"git status"}',
    argumentsTruncated: false,
    suggestedMatcher: { kind: 'command_prefix', value: 'git status' },
    matcherKinds: ['command_prefix', 'any'],
    createdAtMs: 1,
    expiresAtMs: Date.now() + 60_000,
    status: 'pending',
    revision: 1,
    resolution: null,
    ...overrides,
  };
}

export function clarification(
  overrides: Partial<ClarificationRequest> = {},
): ClarificationRequest {
  return {
    id: 'clarify-a',
    agentId: 'agent-a',
    sessionId: 'session-a',
    runId: 'run-a',
    toolCallId: 'tool-q',
    revision: 1,
    createdAtMs: 2,
    status: 'pending',
    response: null,
    questions: [
      {
        id: 'q0',
        question: 'Which target?',
        choices: [
          { id: 'c0', label: 'Local' },
          { id: 'c1', label: 'Remote' },
        ],
        multiSelect: false,
      },
      {
        id: 'q1',
        question: 'Which checks?',
        choices: [
          { id: 'c0', label: 'Build' },
          { id: 'c1', label: 'Test' },
        ],
        multiSelect: true,
      },
      { id: 'q2', question: 'Anything else?', choices: [], multiSelect: false },
    ],
    ...overrides,
  };
}

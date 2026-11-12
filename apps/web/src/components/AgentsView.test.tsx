import { render, screen, within } from '@testing-library/react';
import { expect, it } from 'vitest';

import { AGENT_TALK_TOOL_ACCESS, toolNamesForProfile } from '../lib/agent-access';
import type { AgentDetail } from '../lib/types';
import { AgentsView } from './AgentsView';

function agent(id: string, overrides: Partial<AgentDetail> = {}): AgentDetail {
  return {
    id,
    name: id,
    provider: 'openai',
    model: 'gpt-4.1',
    toolNames: toolNamesForProfile('collaborate'),
    created_at_ms: 1,
    status: 'Idle',
    token_usage: { prompt_tokens: 0, completion_tokens: 0, total_tokens: 0 },
    messages: [],
    ...overrides,
  };
}

it('shows each agent as an equal: solo, talking, or the steward', () => {
  render(
    <AgentsView
      activeAgentId="Ada"
      agents={[
        agent('Ada'),
        agent('Bo', {
          toolNames: [
            ...toolNamesForProfile('collaborate'),
            ...AGENT_TALK_TOOL_ACCESS.tools,
          ],
        }),
        agent('Steward', { isSteward: true }),
      ]}
    />,
  );
  const card = (name: string) => screen.getByRole('article', { name: `${name} agent` });
  expect(within(card('Ada')).getByText('Solo')).toBeInTheDocument();
  expect(within(card('Bo')).getByText('Talks with agents')).toBeInTheDocument();
  expect(within(card('Steward')).getByText('Steward', { selector: 'span' })).toBeInTheDocument();
  expect(screen.queryByText('Main')).not.toBeInTheDocument();
  expect(screen.queryByText('Teammate')).not.toBeInTheDocument();
});

import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { ContextNote } from './SessionContinuity';

describe('context inspector', () => {
  it('exposes estimates and omissions even when history usage is low', () => {
    render(
      <ContextNote
        agentName="Nova"
        usage={{
          estimatedTokens: 5,
          budgetTokens: 10000,
          replyReserveTokens: 500,
          breakdown: {
            conversationTokens: 5,
            summaryTokens: 0,
            checkpointTokens: 2,
            systemInstructionsTokens: 70,
            toolSchemasTokens: 30,
            companionFilesTokens: null,
            memoryTokens: null,
          },
          coverage: {
            basis: 'preview estimate',
            omitted: ['dynamic provider context'],
          },
        }}
      />,
    );
    fireEvent.click(screen.getByText('Inspect context estimates'));
    expect(screen.getByText('System instructions')).toBeVisible();
    expect(screen.getAllByText('Unavailable')).toHaveLength(2);
    expect(screen.getByText(/dynamic provider context/)).toBeVisible();
    expect(
      screen.getByText(/not billed or actual provider usage/),
    ).toBeVisible();
    expect(screen.queryByText(/getting long/)).not.toBeInTheDocument();
  });
});

import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it } from 'vitest';
import type { Session, SessionSummary } from '@animaos/sdk';
import {
  HeaderContextProvider,
  usePublishHeaderDay,
} from '../lib/header-context';
import { HeaderContextLine } from './HeaderContextLine';
import { SessionContinuity } from './sessions/SessionContinuity';

const summary: SessionSummary = {
  text: 'You prefer short summaries with a link to details.',
  throughMessageId: 'm-214',
  createdAtMs: Date.UTC(2026, 9, 15, 9, 30),
  sourceMessageCount: 214,
};

function session(over: Partial<Session> = {}): Session {
  return {
    id: 's1',
    agentId: 'a1',
    title: 'Main',
    kind: 'chat',
    summary,
    ...over,
  } as Session;
}

function Day({ day }: { day: string | null }) {
  usePublishHeaderDay(day);
  return null;
}

describe('header context line', () => {
  it('shows nothing until the conversation publishes something', () => {
    const { container } = render(
      <HeaderContextProvider>
        <HeaderContextLine />
      </HeaderContextProvider>,
    );
    expect(container).toBeEmptyDOMElement();
  });

  it('does not show a day label in the navigation bar', () => {
    const view = render(
      <HeaderContextProvider>
        <HeaderContextLine />
        <Day day="Yesterday" />
      </HeaderContextProvider>,
    );
    expect(screen.queryByText('Yesterday')).not.toBeInTheDocument();
    view.rerender(
      <HeaderContextProvider>
        <HeaderContextLine />
        <Day day="Today" />
      </HeaderContextProvider>,
    );
    expect(screen.queryByText('Today')).not.toBeInTheDocument();
    expect(screen.queryByText('Yesterday')).not.toBeInTheDocument();
  });

  it('takes the kept summary from the session and opens it under the bar', async () => {
    render(
      <HeaderContextProvider>
        <HeaderContextLine />
        <SessionContinuity session={session()} />
      </HeaderContextProvider>,
    );
    // The header owns it: the thread keeps no disclosure of its own.
    expect(screen.queryByText('What anima kept')).not.toBeInTheDocument();
    expect(screen.getByText(/214 earlier messages summarized/)).toBeVisible();
    await userEvent.click(screen.getByRole('button', { name: 'Show summary' }));
    const panel = screen.getByRole('dialog', { name: 'What anima kept' });
    expect(panel).toHaveTextContent('short summaries with a link');
    await userEvent.keyboard('{Escape}');
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Show summary' })).toBeVisible();
  });

  it('leaves the main-chat summary for its inline transcript card', () => {
    render(
      <HeaderContextProvider>
        <HeaderContextLine />
        <SessionContinuity session={session({ mainChat: true })} />
      </HeaderContextProvider>,
    );
    expect(
      screen.queryByText(/earlier messages summarized/),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole('button', { name: 'Show summary' }),
    ).not.toBeInTheDocument();
  });

  it('keeps the summary in the thread when there is no top bar', () => {
    render(<SessionContinuity session={session()} />);
    expect(screen.getByText('What anima kept')).toBeInTheDocument();
  });

  it('clears when the summary goes away', () => {
    const view = render(
      <HeaderContextProvider>
        <HeaderContextLine />
        <SessionContinuity session={session()} />
      </HeaderContextProvider>,
    );
    expect(screen.getByText(/earlier messages summarized/)).toBeVisible();
    view.rerender(
      <HeaderContextProvider>
        <HeaderContextLine />
        <SessionContinuity session={session({ summary: undefined })} />
      </HeaderContextProvider>,
    );
    expect(
      screen.queryByText(/earlier messages summarized/),
    ).not.toBeInTheDocument();
  });
});

import { fireEvent, render } from '@testing-library/react';
import { expect, it } from 'vitest';
import { AgentAvatar } from './AgentAvatar';

it('shows the supplied presence fallback after an image failure and retries on avatar refresh', () => {
  const view = render(
    <AgentAvatar id="nova" name="Nova" fallback={<span>Presence</span>} />,
  );
  fireEvent.error(view.container.querySelector('img')!);
  expect(view.getByText('Presence')).toBeVisible();
  expect(view.container.querySelector('img')).toBeNull();

  fireEvent(window, new CustomEvent('agent-avatar-changed', { detail: 'other' }));
  expect(view.getByText('Presence')).toBeVisible();
  fireEvent(window, new CustomEvent('agent-avatar-changed', { detail: 'nova' }));
  expect(view.queryByText('Presence')).toBeNull();
  expect(view.container.querySelector('img')).toHaveAttribute(
    'src',
    expect.stringContaining('/api/agents/nova/avatar?v='),
  );
});

it('tries the new agent image when the previous agent used the fallback', () => {
  const view = render(
    <AgentAvatar id="nova" name="Nova" fallback={<span>Presence</span>} />,
  );
  fireEvent.error(view.container.querySelector('img')!);
  view.rerender(
    <AgentAvatar id="scout" name="Scout" fallback={<span>Presence</span>} />,
  );
  expect(view.queryByText('Presence')).toBeNull();
  expect(view.container.querySelector('img')).toHaveAttribute(
    'src',
    '/api/agents/scout/avatar?v=0',
  );
});

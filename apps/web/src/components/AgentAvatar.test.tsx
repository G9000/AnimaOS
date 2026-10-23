import { fireEvent, render, waitFor } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import { AgentAvatar } from './AgentAvatar';
import { clearOwnerApiKey, setOwnerApiKey } from '../lib/owner-api-key';

afterEach(() => { clearOwnerApiKey(); vi.unstubAllGlobals(); });

it('uses authenticated blobs and releases the prior image when the agent changes', async () => {
  setOwnerApiKey('owner-secret');
  const fetch = vi.fn().mockImplementation(async () => new Response(new Uint8Array([1]), { headers: { 'content-type': 'image/png' } }));
  vi.stubGlobal('fetch', fetch);
  const createObjectURL = vi.fn().mockReturnValueOnce('blob:old-agent').mockReturnValueOnce('blob:new-agent');
  const revokeObjectURL = vi.fn();
  Object.defineProperties(URL, { createObjectURL: { configurable: true, value: createObjectURL }, revokeObjectURL: { configurable: true, value: revokeObjectURL } });
  const view = render(<AgentAvatar id="nova" name="Nova" fallback={<span>Presence</span>} />);
  await waitFor(() => expect(view.container.querySelector('img')).toHaveAttribute('src', 'blob:old-agent'));
  expect(fetch.mock.calls[0][1].headers).toMatchObject({ 'x-api-key': 'owner-secret' });
  expect(fetch.mock.calls[0][0]).toBe('/api/agents/nova/avatar?v=0');
  view.rerender(<AgentAvatar id="scout" name="Scout" fallback={<span>Presence</span>} />);
  await waitFor(() => expect(view.container.querySelector('img')).toHaveAttribute('src', 'blob:new-agent'));
  expect(revokeObjectURL).toHaveBeenCalledWith('blob:old-agent');
  view.unmount();
  expect(revokeObjectURL).toHaveBeenCalledWith('blob:new-agent');
});

it('shows the supplied presence fallback after an image failure and retries on avatar refresh', () => {
  const view = render(
    <AgentAvatar id="nova" name="Nova" fallback={<span>Presence</span>} />,
  );
  fireEvent.error(view.container.querySelector('img')!);
  expect(view.getByText('Presence')).toBeVisible();
  expect(view.container.querySelector('img')).toBeNull();

  fireEvent(
    window,
    new CustomEvent('agent-avatar-changed', { detail: 'other' }),
  );
  expect(view.getByText('Presence')).toBeVisible();
  fireEvent(
    window,
    new CustomEvent('agent-avatar-changed', { detail: 'nova' }),
  );
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

it('uses the design presence orb when no uploaded avatar exists', () => {
  const view = render(<AgentAvatar id="nova" name="Nova" size={72} />);
  fireEvent.error(view.container.querySelector('img')!);
  expect(view.container.querySelector('.halo-orb')).toBeInTheDocument();
});

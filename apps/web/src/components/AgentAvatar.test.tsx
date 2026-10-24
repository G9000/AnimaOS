import { fireEvent, render, waitFor } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import { AgentAvatar } from './AgentAvatar';
import { clearOwnerApiKey, setOwnerApiKey } from '../lib/owner-api-key';

afterEach(() => { localStorage.clear(); clearOwnerApiKey(); vi.unstubAllGlobals(); });

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

function authenticatedImages() {
  localStorage.setItem('anima.consoleSession', 'signed-header');
  const fetch = vi.fn().mockImplementation(async () => new Response(new Uint8Array([1]), { headers: { 'content-type': 'image/png' } }));
  vi.stubGlobal('fetch', fetch);
  let index = 0;
  Object.defineProperties(URL, { createObjectURL: { configurable: true, value: vi.fn(() => 'blob:avatar-' + (++index)) }, revokeObjectURL: { configurable: true, value: vi.fn() } });
  return fetch;
}

it('shows the supplied presence fallback after an image failure and retries on avatar refresh', async () => {
  const fetch = authenticatedImages();
  const view = render(<AgentAvatar id="nova" name="Nova" fallback={<span>Presence</span>} />);
  await waitFor(() => expect(view.container.querySelector('img')).toHaveAttribute('src', 'blob:avatar-1'));
  fireEvent.error(view.container.querySelector('img')!);
  expect(view.getByText('Presence')).toBeVisible();
  expect(view.container.querySelector('img')).toBeNull();
  fireEvent(window, new CustomEvent('agent-avatar-changed', { detail: 'other' }));
  expect(view.getByText('Presence')).toBeVisible();
  expect(fetch).toHaveBeenCalledTimes(1);
  fireEvent(window, new CustomEvent('agent-avatar-changed', { detail: 'nova' }));
  await waitFor(() => expect(view.container.querySelector('img')).toHaveAttribute('src', 'blob:avatar-2'));
  expect(view.queryByText('Presence')).toBeNull();
  expect(fetch.mock.calls[1][0]).toContain('/api/agents/nova/avatar?v=');
});

it('tries the new agent image when the previous agent used the fallback', async () => {
  const fetch = authenticatedImages();
  const view = render(<AgentAvatar id="nova" name="Nova" fallback={<span>Presence</span>} />);
  await waitFor(() => expect(view.container.querySelector('img')).toHaveAttribute('src', 'blob:avatar-1'));
  fireEvent.error(view.container.querySelector('img')!);
  view.rerender(<AgentAvatar id="scout" name="Scout" fallback={<span>Presence</span>} />);
  await waitFor(() => expect(view.container.querySelector('img')).toHaveAttribute('src', 'blob:avatar-2'));
  expect(view.queryByText('Presence')).toBeNull();
  expect(fetch.mock.calls[1][0]).toBe('/api/agents/scout/avatar?v=0');
});

it('uses the design presence orb when no authenticated uploaded avatar is available', () => {
  const view = render(<AgentAvatar id="nova" name="Nova" size={72} />);
  expect(view.container.querySelector('img')).toBeNull();
  expect(view.container.querySelector('.halo-orb')).toBeInTheDocument();
});

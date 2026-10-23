import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, expect, it, vi } from 'vitest';
import { OwnerApiKeyGate } from './OwnerApiKeyGate';
import { clearOwnerApiKey, ownerApiKeyHeaders } from '../lib/owner-api-key';

afterEach(() => { clearOwnerApiKey(); vi.unstubAllGlobals(); });

it('does not mount owner children until access is verified', async () => {
  let resolve!: (value: Response) => void;
  const pending = new Promise<Response>((done) => { resolve = done; });
  const fetch = vi.fn(() => pending);
  vi.stubGlobal('fetch', fetch);
  render(<OwnerApiKeyGate><p>Owner workspace</p></OwnerApiKeyGate>);
  expect(screen.queryByText('Owner workspace')).not.toBeInTheDocument();
  resolve(Response.json({}));
  expect(await screen.findByText('Owner workspace')).toBeInTheDocument();
  // A launch URL can still contain its one-time login code before the owner
  // unlocks this gate. The preflight must send only its origin as referrer.
  expect(fetch).toHaveBeenCalledWith('/api/capabilities', expect.objectContaining({ method: 'GET', credentials: 'same-origin', cache: 'no-store', redirect: 'error', referrerPolicy: 'origin' }));
});

it('verifies a submitted key before granting memory-only owner transport', async () => {
  let resolve!: (value: Response) => void;
  const pending = new Promise<Response>((done) => { resolve = done; });
  const fetch = vi.fn().mockResolvedValueOnce(Response.json({}, { status: 401 })).mockReturnValueOnce(pending);
  vi.stubGlobal('fetch', fetch);
  render(<OwnerApiKeyGate><p>Owner workspace</p></OwnerApiKeyGate>);
  fireEvent.change(await screen.findByLabelText('Owner API key'), { target: { value: 'owner-secret' } });
  fireEvent.click(screen.getByRole('button', { name: 'Unlock owner console' }));
  expect(ownerApiKeyHeaders()).toEqual({});
  expect(fetch.mock.calls[1][1].headers).toMatchObject({ 'x-api-key': 'owner-secret' });
  resolve(Response.json({}));
  expect(await screen.findByText('Owner workspace')).toBeInTheDocument();
  expect(ownerApiKeyHeaders()).toEqual({ 'x-api-key': 'owner-secret' });
  expect(window.location.href).not.toContain('owner-secret');
});

it('keeps forbidden local-owner access outside the workspace', async () => {
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue(Response.json({}, { status: 403 })));
  render(<OwnerApiKeyGate><p>Owner workspace</p></OwnerApiKeyGate>);
  expect(await screen.findByRole('alert')).toHaveTextContent('not approved as the local owner');
  expect(screen.queryByText('Owner workspace')).not.toBeInTheDocument();
});

it('cannot restore owner authority from a late login after unmount', async () => {
  let resolve!: (value: Response) => void;
  const pending = new Promise<Response>((done) => { resolve = done; });
  vi.stubGlobal('fetch', vi.fn().mockResolvedValueOnce(Response.json({}, { status: 401 })).mockReturnValueOnce(pending));
  const { unmount } = render(<OwnerApiKeyGate><p>Owner workspace</p></OwnerApiKeyGate>);
  fireEvent.change(await screen.findByLabelText('Owner API key'), { target: { value: 'owner-secret' } });
  fireEvent.click(screen.getByRole('button', { name: 'Unlock owner console' }));
  unmount();
  resolve(Response.json({}));
  await waitFor(() => expect(ownerApiKeyHeaders()).toEqual({}));
});

import { afterEach, expect, it } from 'vitest';
import { clearOwnerApiKey, ownerApiKeyHeaders, setOwnerApiKey } from './owner-api-key';

afterEach(clearOwnerApiKey);

it('keeps the owner key in memory and clears authority explicitly', () => {
  const before = { local: localStorage.length, session: sessionStorage.length, url: window.location.href };
  expect(ownerApiKeyHeaders()).toEqual({});
  setOwnerApiKey('owner-secret');
  expect(ownerApiKeyHeaders()).toEqual({ 'x-api-key': 'owner-secret' });
  expect(localStorage.length).toBe(before.local);
  expect(sessionStorage.length).toBe(before.session);
  expect(window.location.href).toBe(before.url);
  clearOwnerApiKey();
  expect(ownerApiKeyHeaders()).toEqual({});
});

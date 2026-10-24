import { afterEach, describe, expect, it, vi } from 'vitest';

import {
  CONSOLE_SESSION_HEADER,
  LAUNCH_CODE_EXPIRED_WARNING,
  LAUNCH_CODE_FAILED_WARNING,
  LAUNCH_CODE_SPENT_WARNING,
  claimConsoleSession,
  consoleSessionHeaders,
} from './console-session';
import { daemonRequestHeaders } from './daemon-api';

afterEach(() => {
  window.localStorage.clear();
  window.history.replaceState(null, '', '/');
});

describe('console session', () => {
  it('trades the launch code for a token and takes the code out of the address', async () => {
    window.history.replaceState(null, '', '/?anima-login=1.abc.def&x=1#/settings');
    const fetchMock = vi
      .fn<typeof fetch>()
      .mockResolvedValue(Response.json({ token: 'session-token' }));

    expect(await claimConsoleSession(window, fetchMock)).toEqual({
      kind: 'opened',
    });

    expect(window.location.search).toBe('?x=1');
    expect(window.location.hash).toBe('#/settings');
    expect(fetchMock).toHaveBeenCalledWith('/api/system/session', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      credentials: 'same-origin',
      redirect: 'error',
      cache: 'no-store',
      referrerPolicy: 'origin',
      body: JSON.stringify({ code: '1.abc.def' }),
    });
    expect(consoleSessionHeaders()).toEqual({
      [CONSOLE_SESSION_HEADER]: 'session-token',
    });
    expect(daemonRequestHeaders()).toEqual({
      [CONSOLE_SESSION_HEADER]: 'session-token',
    });
  });

  it('does nothing without a launch code', async () => {
    const fetchMock = vi.fn<typeof fetch>();
    expect(await claimConsoleSession(window, fetchMock)).toEqual({
      kind: 'none',
    });
    expect(fetchMock).not.toHaveBeenCalled();
    expect(daemonRequestHeaders()).toEqual({});
  });

  it.each([
    [409, 'launch_code_spent', LAUNCH_CODE_SPENT_WARNING],
    [403, 'launch_code_expired', LAUNCH_CODE_EXPIRED_WARNING],
    [403, undefined, LAUNCH_CODE_FAILED_WARNING],
  ])(
    'keeps no token and warns the owner when the daemon refuses the code (%i %s)',
    async (status, code, warning) => {
      window.history.replaceState(null, '', '/?anima-login=1.abc.def');
      const fetchMock = vi
        .fn<typeof fetch>()
        .mockResolvedValue(Response.json({ error: 'refused', code }, { status }));
      expect(await claimConsoleSession(window, fetchMock)).toEqual({
        kind: 'refused',
        warning,
      });
      expect(window.location.search).toBe('');
      expect(consoleSessionHeaders()).toEqual({});
    },
  );

  it('says a spent code may mean someone else opened a session', () => {
    expect(LAUNCH_CODE_SPENT_WARNING).toContain('already used');
    expect(LAUNCH_CODE_SPENT_WARNING).toContain(
      'someone else may have opened a console session',
    );
  });
});

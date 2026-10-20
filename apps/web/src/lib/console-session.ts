// The console session that lets this console stop, restart or update Anima
// (hosts/rust-daemon/src/console_session.rs). An Origin header proves nothing:
// any local process can send one. So the tray and `anima` open the console
// with a one-time `anima-login` code, which the console trades for a session
// in two halves: an HttpOnly cookie the daemon sets, and a token kept in this
// origin's storage, which no page on another port can read.

export const CONSOLE_LOGIN_PARAM = 'anima-login';
export const CONSOLE_SESSION_HEADER = 'x-anima-console-session';
const STORAGE_KEY = 'anima.consoleSession';

type ConsoleWindow = Pick<Window, 'location' | 'history' | 'localStorage'>;

/** How trading the address's launch code went. `warning` is for the owner. */
export type ConsoleSessionClaim =
  | { kind: 'none' }
  | { kind: 'opened' }
  | { kind: 'refused'; warning: string };

export const LAUNCH_CODE_SPENT_WARNING =
  'This launch link was already used — someone else may have opened a console session. Open Anima again from the tray or `anima`.';
export const LAUNCH_CODE_EXPIRED_WARNING =
  'This launch link has expired. Open Anima again from the tray or `anima` to use the Anima app controls.';
export const LAUNCH_CODE_FAILED_WARNING =
  'This launch link did not open a console session. Open Anima again from the tray or `anima` to use the Anima app controls.';

function storage(win: Pick<Window, 'localStorage'>): Storage | null {
  try {
    return win.localStorage;
  } catch {
    return null;
  }
}

/** The session's token half, when this console has one. */
export function consoleSessionToken(
  win: Pick<Window, 'localStorage'> = window,
): string | null {
  try {
    return storage(win)?.getItem(STORAGE_KEY) || null;
  } catch {
    return null;
  }
}

export function consoleSessionHeaders(
  win: Pick<Window, 'localStorage'> = window,
): Record<string, string> {
  const token = consoleSessionToken(win);
  return token ? { [CONSOLE_SESSION_HEADER]: token } : {};
}

async function refusal(response: Response): Promise<ConsoleSessionClaim> {
  let code: unknown;
  try {
    code = ((await response.json()) as { code?: unknown }).code;
  } catch {
    code = undefined;
  }
  const warning =
    code === 'launch_code_spent'
      ? LAUNCH_CODE_SPENT_WARNING
      : code === 'launch_code_expired'
        ? LAUNCH_CODE_EXPIRED_WARNING
        : LAUNCH_CODE_FAILED_WARNING;
  return { kind: 'refused', warning };
}

/** Trades a launch code in the address for a session. The code leaves the
 *  address (and so the history) first: it is a secret, and spent once used.
 *  A refused code comes back with a warning for the owner: a code someone
 *  else already traded may mean they hold a session. */
export async function claimConsoleSession(
  win: ConsoleWindow = window,
  fetchImpl: typeof fetch = (...args) => globalThis.fetch(...args),
): Promise<ConsoleSessionClaim> {
  const url = new URL(win.location.href);
  const code = url.searchParams.get(CONSOLE_LOGIN_PARAM);
  if (code === null) return { kind: 'none' };
  url.searchParams.delete(CONSOLE_LOGIN_PARAM);
  win.history.replaceState(win.history.state, '', url.toString());
  try {
    const response = await fetchImpl('/api/system/session', {
      method: 'POST',
      headers: { 'content-type': 'application/json' },
      credentials: 'same-origin',
      body: JSON.stringify({ code }),
    });
    if (!response.ok) return await refusal(response);
    const { token } = (await response.json()) as { token?: unknown };
    if (typeof token !== 'string' || token === '') {
      return { kind: 'refused', warning: LAUNCH_CODE_FAILED_WARNING };
    }
    storage(win)?.setItem(STORAGE_KEY, token);
    return { kind: 'opened' };
  } catch {
    return { kind: 'refused', warning: LAUNCH_CODE_FAILED_WARNING };
  }
}

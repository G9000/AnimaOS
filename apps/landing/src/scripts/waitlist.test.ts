// @vitest-environment jsdom
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

const track = vi.fn();

async function submitSignup(ok = true) {
  vi.stubGlobal('fetch', vi.fn().mockResolvedValue({
    ok,
    json: async () => ok ? {} : { error: 'Waitlist unavailable' },
  }));
  await import('./waitlist');
  document.getElementById('wl-form')!.dispatchEvent(new Event('submit', { cancelable: true }));
  await vi.waitFor(() => {
    expect(document.querySelector('.joined') || document.querySelector('.err')).not.toBeNull();
  });
}

describe('waitlist analytics', () => {
  beforeEach(() => {
    vi.resetModules();
    track.mockReset();
    localStorage.clear();
    document.body.innerHTML = `
      <div id="wl-slot"><form id="wl-form">
        <input id="wl-email" value="person@example.com">
        <input name="website" value="">
        <button id="wl-submit"></button><p id="wl-status"></p>
      </form></div>`;
    Object.assign(window, { umami: { track } });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
    Reflect.deleteProperty(window, 'umami');
  });

  it('records one signup event after the server accepts the signup', async () => {
    await submitSignup();
    expect(track).toHaveBeenCalledExactlyOnceWith('waitlist-signup');
    expect(document.querySelector('.joined')?.textContent).toContain('person@example.com');
  });

  it('does not record a signup when the server refuses it', async () => {
    await submitSignup(false);
    expect(track).not.toHaveBeenCalled();
  });

  it('keeps signup working when the tracker is unavailable', async () => {
    Reflect.deleteProperty(window, 'umami');
    await submitSignup();
    expect(document.querySelector('.joined')).not.toBeNull();
  });

  it.each(['throw', 'reject'])('keeps signup successful when analytics fails: %s', async (failure) => {
    track.mockImplementation(() => {
      if (failure === 'throw') throw new Error('Analytics blocked');
      return Promise.reject(new Error('Analytics blocked'));
    });
    await submitSignup();
    expect(track).toHaveBeenCalledOnce();
    expect(document.querySelector('.err')).toBeNull();
  });
});

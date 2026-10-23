/* waitlist: posts to /api/waitlist, which keeps the Airtable key on the server */
// eslint-disable-next-line unicorn/require-module-specifiers -- Keep this browser script isolated from DOM globals.
export {};

const slot = document.getElementById('wl-slot') as HTMLElement;
const form = document.getElementById('wl-form') as HTMLFormElement;
const input = document.getElementById('wl-email') as HTMLInputElement;
const status = document.getElementById('wl-status') as HTMLElement;
const submit = document.getElementById('wl-submit') as HTMLButtonElement;
const KEY = 'anima.waitlist';

const say = (m: string, err = false) => {
  status.textContent = m;
  status.classList.toggle('err', err);
};
const remember = (email: string | null) => {
  try {
    if (email) localStorage.setItem(KEY, email);
    else localStorage.removeItem(KEY);
  } catch {}
};

const showForm = () => {
  slot.replaceChildren(form);
  submit.disabled = false;
};
const showJoined = (email: string) => {
  const box = document.createElement('div');
  box.className = 'joined';
  const k = document.createElement('span');
  k.className = 'mono pink';
  k.textContent = 'Waitlist · you’re in';
  const h = document.createElement('h3');
  h.textContent = `We’ll email ${email} when your invite is ready.`;
  const acts = document.createElement('div');
  acts.className = 'row';
  const again = document.createElement('button');
  again.type = 'button';
  again.className = 'btn';
  again.textContent = 'Use another email';
  again.addEventListener('click', () => {
    remember(null);
    form.reset();
    say('');
    showForm();
    input.focus();
  });
  acts.append(again);
  box.append(k, h, acts);
  slot.replaceChildren(box);
};

form.addEventListener('submit', async (e) => {
  e.preventDefault();
  const email = input.value.trim();
  if (!/^[^\s@]+@[^\s@]+\.[^\s@]+$/.test(email)) {
    say('Enter an email address like you@domain.com.', true);
    input.focus();
    return;
  }
  submit.disabled = true;
  say('Saving…');
  const website =
    (form.elements.namedItem('website') as HTMLInputElement | null)?.value ??
    '';
  try {
    const res = await fetch('/api/waitlist', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({
        email,
        website,
        source:
          new URLSearchParams(location.search).get('ref') ?? document.referrer,
      }),
    });
    const data = await res.json().catch(() => ({}));
    if (!res.ok)
      throw new Error(
        data.error || 'Couldn’t save that just now. Try again in a moment.',
      );
    remember(email);
    say('');
    showJoined(email);
    // Analytics must never turn an accepted signup into an error.
    try {
      const umami = (
        window as Window & { umami?: { track: (name: string) => unknown } }
      ).umami;
      Promise.resolve(umami?.track('waitlist-signup')).catch(() => {});
    } catch {}
  } catch (err) {
    submit.disabled = false;
    say(
      err instanceof Error && err.message !== 'Failed to fetch'
        ? err.message
        : 'Couldn’t reach the server. Try again in a moment.',
      true,
    );
  }
});

try {
  const saved = localStorage.getItem(KEY);
  if (saved) showJoined(saved);
} catch {}

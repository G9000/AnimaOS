import { describe, it, expect, vi } from 'vitest';
import { createDaemonClient } from './client.js';

describe('chat capabilities', () => {
  it('encodes chat scope and forwards cancellation', async () => {
    const fetch = vi.fn().mockResolvedValue(new Response('{}', { status: 200 }));
    const client = createDaemonClient({ baseUrl: '', fetch });
    const signal = new AbortController().signal;
    await client.chatCapabilities.get('a/b', 'chat:one/two', { signal });
    expect(fetch).toHaveBeenCalledWith('/api/agents/a%2Fb/sessions/chat%3Aone%2Ftwo/capabilities', expect.objectContaining({ signal }));
  });
});

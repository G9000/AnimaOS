import { describe, expect, it } from 'vitest';
import { createDaemonClient } from './index.js';

const input = {
  address: 'agent@example.com',
  imap: {
    host: 'imap.example.com',
    port: 993,
    security: 'tls' as const,
    username: 'inbox-user',
  },
  smtp: {
    host: 'smtp.example.com',
    port: 587,
    security: 'starttls' as const,
    username: 'send-user',
  },
  imapPassword: 'receiving-secret',
  smtpPassword: 'sending-secret',
  autoReply: {
    enabled: true,
    allowedSenders: ['friend@example.com'],
    authservId: 'mx.example.com',
  },
};

describe('generic email transport', () => {
  it('sets up email without OAuth and preserves the connector envelope', async () => {
    const requests: { url: string; init?: RequestInit }[] = [];
    const connector = {
      id: 'email-1',
      type: 'email',
      email: {
        address: input.address,
        imap: input.imap,
        smtp: input.smtp,
        autoReply: input.autoReply,
      },
    };
    const client = createDaemonClient({
      baseUrl: '',
      fetch: async (url, init) => {
        requests.push({ url: String(url), init });
        return Response.json({ connector });
      },
    });
    expect(
      await client.connectors.connectGenericEmail('agent /?', input),
    ).toEqual({ connector });
    expect(requests).toHaveLength(1);
    expect(requests[0].url).toBe(
      '/api/agents/agent%20%2F%3F/connectors/mail/email',
    );
    expect(requests[0].init?.method).toBe('POST');
    expect(JSON.parse(requests[0].init?.body as string)).toEqual(input);
  });

  it('uses the existing inbox and immutable draft approval routes for email', async () => {
    const requests: { url: string; init?: RequestInit }[] = [];
    const draft = { id: 'draft /?', state: 'pending' };
    const client = createDaemonClient({
      baseUrl: '',
      fetch: async (url, init) => {
        requests.push({ url: String(url), init });
        return Response.json({
          configured: true,
          connector: null,
          messages: [],
          drafts: [draft],
          draft,
        });
      },
    });
    const api = client.connectors;
    await api.mailStatus('a', 'email');
    await api.mailMessages('a', 'email', 'c /?', { refresh: true });
    await api.mailDrafts('a', 'email', 'c /?');
    await api.createMailDraft('a', 'email', 'c /?', {
      to: ['friend@example.com'],
      subject: 'Review',
      body: 'Message',
    });
    expect(requests).toHaveLength(4);
    await api.approveMailDraft('a', 'email', 'c /?', 'd /?');
    await api.rejectMailDraft('a', 'email', 'c /?', 'd /?');
    await api.disconnectMail('a', 'email', 'c /?');
    const prefix = '/api/agents/a/connectors/mail/email/c%20%2F%3F';
    expect(requests.map(({ url }) => url)).toEqual([
      '/api/agents/a/connectors/mail/email',
      `${prefix}/messages?refresh=true`,
      `${prefix}/drafts`,
      `${prefix}/drafts`,
      `${prefix}/drafts/d%20%2F%3F/approve`,
      `${prefix}/drafts/d%20%2F%3F/reject`,
      prefix,
    ]);
    expect(requests.slice(3).map(({ init }) => init?.method)).toEqual([
      'POST',
      'POST',
      'POST',
      'DELETE',
    ]);
  });

  it('does not replay a failed credential setup', async () => {
    let attempts = 0;
    const client = createDaemonClient({
      baseUrl: '',
      fetch: async () => {
        attempts++;
        return Response.json({ error: 'email_unavailable' }, { status: 503 });
      },
    });
    await expect(
      client.connectors.connectGenericEmail('a', input),
    ).rejects.toMatchObject({ status: 503 });
    expect(attempts).toBe(1);
  });
});

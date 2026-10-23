import { useEffect, useState } from 'react';
import type { ChatCapabilities, ChatCapabilitiesClient } from '@animaos/sdk';
import { chatCapabilitiesApi } from '../../lib/daemon-api';

export function ChatToolsPanel({ agentId, sessionId, refreshKey, api = chatCapabilitiesApi }: {
  agentId: string; sessionId: string; refreshKey?: string | number;
  api?: Pick<ChatCapabilitiesClient, 'get'>;
}) {
  const [view, setView] = useState<ChatCapabilities | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [query, setQuery] = useState('');
  const [refresh, setRefresh] = useState(0);
  useEffect(() => {
    let ignore = false;
    const controller = new AbortController();
    setView(null);
    setError(null);
    api.get(agentId, sessionId, { signal: controller.signal }).then(result => {
      if (!ignore && result.agentId === agentId && result.sessionId === sessionId) setView(result);
    }).catch(caught => {
      if (!ignore) setError(caught instanceof Error ? caught.message : String(caught));
    });
    return () => { ignore = true; controller.abort(); };
  }, [agentId, sessionId, refresh, refreshKey, api]);
  const scoped = view?.agentId === agentId && view.sessionId === sessionId ? view : null;
  const filtered = scoped?.tools.filter(tool => `${tool.name} ${tool.description}`.toLowerCase().includes(query.toLowerCase())) ?? [];
  return <section className="space-y-3 p-3 text-sm" aria-label="Current chat tools">
    <div className="flex items-center justify-between gap-3">
      <strong>Tools for this chat</strong>
      <button type="button" onClick={() => setRefresh(n => n + 1)}>Refresh tools</button>
    </div>
    {error && <p role="alert">{error}</p>}
    {!scoped && !error && <p role="status">Reading current tool scope…</p>}
    {scoped && <>
      <p>{scoped.basis === 'active_turn' ? 'Running turn · captured scope with current grants checked' : 'Next owner turn · current scope'}</p>
      {scoped.preparing && <p role="status">The running turn is preparing its tools.</p>}
      <p className="text-xs text-ink-3">{scoped.notice}</p>
      <label className="block">Find a tool <input className="ml-2 bg-transparent" value={query} onChange={event => setQuery(event.target.value)} /></label>
      <ul className="max-h-72 space-y-2 overflow-y-auto">
        {filtered.map(tool => <li key={tool.name} className="rounded border border-white/10 p-2">
          <div><code>{tool.name}</code> · {tool.origin} · {tool.risk}</div>
          <p>{tool.description}</p>
          <p className="text-xs text-ink-3">{tool.available ? 'In scope' : 'Unavailable'} · policy: {tool.approval === 'ask' ? 'asks for approval' : tool.approval}{tool.argumentDependent ? ' · depends on arguments and safeguards' : ''}</p>
          {tool.reason && <p>{tool.reason}</p>}
        </li>)}
      </ul>
      {filtered.length === 0 && <p>No tools match this view.</p>}
      {scoped.connections.length > 0 && <div><strong>MCP connections</strong><ul>{scoped.connections.map(connection => <li key={connection.id}>{connection.label}: {connection.state}</li>)}</ul></div>}
    </>}
  </section>;
}

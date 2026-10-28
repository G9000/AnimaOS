import { useEffect, useRef, useState } from 'react';
import { createDaemonClient } from '@animaos/sdk';
import { ownerFetch } from '../lib/owner-fetch';
import { useRenderScope } from '../hooks/useRenderScope';
import { ServiceRow } from './ServiceRow';
import { BrandIcon } from './BrandIcon';
import { OAuthSetupLink } from './OAuthSetupLink';

const FILES_SUMMARY = 'OneDrive and SharePoint';

type Mode = 'oneDrive' | 'sharePoint';
interface Connection { id: string; mode: Mode; status: string; generation: string; selectionRevision: number; accountLabel: string | null; approvedSiteUrl: string | null; selectedFiles: { key: string; name: string }[] }
interface Choice { key: string; name: string; isFolder: boolean; mime: string | null }
export interface MicrosoftFilesApi {
  status(agent: string, options?: { signal?: AbortSignal }): Promise<{ configured: boolean; connectors: Connection[] }>;
  connect(agent: string, input: { mode: Mode; approvedSiteUrl?: string | null }): Promise<{ connectorId: string; consentUrl: string; expiresAtMs: number }>;
  files(agent: string, id: string, input?: { folderKey?: string; cursor?: string }, options?: { signal?: AbortSignal }): Promise<{ files: Choice[]; nextCursor: string | null }>;
  select(agent: string, id: string, input: { expectedGeneration: string; expectedSelectionRevision: number; fileKeys: string[] }): Promise<Connection>;
  disconnect(agent: string, id: string, input: { expectedGeneration: string }): Promise<{ disconnected: boolean }>;
}
const button = 'rounded-lg border border-line px-3 py-2 text-xs hover:bg-white/[0.04] disabled:opacity-50';
const defaultApi = (createDaemonClient({ baseUrl: '', fetch: ownerFetch }) as unknown as { microsoftFiles: MicrosoftFilesApi }).microsoftFiles;

export function MicrosoftFilesCard({ agentId, online = true, epoch = 0, disabled = false, api = defaultApi }: {
  agentId: string; online?: boolean; epoch?: number; disabled?: boolean; api?: MicrosoftFilesApi;
}) {
  const scope = useRenderScope(agentId, online, epoch);
  return online
    ? <MicrosoftFilesContents key={scope.generation} agentId={agentId} disabled={disabled} api={api} />
    : <ServiceRow id="microsoft-files" group="microsoft" title="Microsoft files" summary={FILES_SUMMARY} icon={<BrandIcon brand="onedrive" />} status="Offline"><p role="status">Reconnect to review files.</p></ServiceRow>;
}

function MicrosoftFilesContents({ agentId, disabled, api }: { agentId: string; disabled: boolean; api: MicrosoftFilesApi }) {
  const [status, setStatus] = useState<{ configured: boolean; connectors: Connection[] } | null>(null);
  const [mode, setMode] = useState<Mode>('oneDrive');
  const [site, setSite] = useState('');
  const [consent, setConsent] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [choices, setChoices] = useState<Choice[]>([]);
  const [selected, setSelected] = useState<string[]>([]);
  const [folder, setFolder] = useState<string | undefined>();
  const [cursor, setCursor] = useState<string | null>(null);
  const alive = useRef(false);
  const inFlight = useRef(false);
  const active = status?.connectors.find((record) => record.id === activeId) ?? null;
  useEffect(() => {
    alive.current = true;
    const controller = new AbortController();
    void api.status(agentId, { signal: controller.signal }).then((result) => { if (alive.current) setStatus(result); }).catch(() => { if (alive.current) setError('Could not load Microsoft files.'); });
    return () => { alive.current = false; controller.abort(); };
  }, [agentId, api]);
  async function run(action: () => Promise<void>) {
    if (inFlight.current || disabled) return;
    inFlight.current = true; setBusy(true); setError(null);
    try { await action(); } catch (cause) { if (alive.current) setError(cause instanceof Error ? cause.message : 'Could not complete this action.'); }
    finally { inFlight.current = false; if (alive.current) setBusy(false); }
  }
  async function reload() { const next = await api.status(agentId); if (alive.current) { setStatus(next); setConsent(null); } }
  async function loadFiles(record: Connection, nextFolder?: string, nextCursor?: string) {
    const page = await api.files(agentId, record.id, { folderKey: nextFolder, cursor: nextCursor });
    if (alive.current) { setActiveId(record.id); setFolder(nextFolder); setCursor(page.nextCursor); setChoices((previous) => nextCursor ? [...previous, ...page.files] : page.files); }
  }
  function validConsent(value: string): string | null {
    try { const url = new URL(value); return url.protocol === 'https:' && url.hostname === 'login.microsoftonline.com' && !url.username && !url.password && !url.port ? url.href : null; } catch { return null; }
  }
  const label = !status ? (error ? 'Status unavailable' : '')
    : !status.configured ? 'Setup required'
    : status.connectors.some((record) => record.status === 'active') ? 'Connected'
    : status.connectors.some((record) => record.status !== 'disconnected') ? 'Awaiting authorization'
    : 'Not connected';
  return <ServiceRow id="microsoft-files" group="microsoft" title="Microsoft files" summary={FILES_SUMMARY} icon={<BrandIcon brand="onedrive" />} status={label}><div className="space-y-3 text-xs">
    <p className="text-ink-3">Pick OneDrive or SharePoint files to search. SharePoint needs a site your organization has approved.</p>
    {error && <p role="alert" className="text-danger">{error}</p>}
    {!status && !error && <p role="status">Loading Microsoft files…</p>}
    {status && !status.configured && <OAuthSetupLink provider="microsoft" />}
    {status?.connectors.filter((record) => record.status !== 'disconnected').map((record) => <article key={record.id} className="space-y-2 border-t border-line pt-2">
      <p><strong>{record.mode === 'oneDrive' ? 'OneDrive' : 'SharePoint'}</strong> · {record.accountLabel || 'Account'} · {record.status === 'active' ? 'Connected' : record.status === 'pairing' ? 'Awaiting authorization' : 'Reconnect required'}</p>
      {record.selectedFiles.length > 0 && <p className="text-ink-3">Selected: {record.selectedFiles.map((file) => file.name).join(', ')}</p>}
      {record.status === 'active' && <button type="button" className={button} disabled={disabled || busy} onClick={() => void run(async () => { setSelected(record.selectedFiles.map((file) => file.key)); await loadFiles(record); })}>Choose {record.mode === 'oneDrive' ? 'OneDrive' : 'SharePoint'} files</button>}
      <button type="button" className={button} disabled={disabled || busy} onClick={() => void run(async () => { setChoices([]); setActiveId(null); await api.disconnect(agentId, record.id, { expectedGeneration: record.generation }); await reload(); })}>Disconnect {record.mode === 'oneDrive' ? 'OneDrive' : 'SharePoint'}</button>
    </article>)}
    {active && <div className="space-y-2 rounded-lg border border-line p-3" aria-label="Choose Microsoft files">
      <p>Select up to 16 files. Only selected files are indexed.</p>
      {folder && <button type="button" className={button} disabled={busy || disabled} onClick={() => void run(() => loadFiles(active))}>Back to top</button>}
      {choices.map((choice) => choice.isFolder ? <button key={choice.key} type="button" className={`${button} block`} disabled={busy || disabled} onClick={() => void run(() => loadFiles(active, choice.key))}>Open {choice.name}</button> : <label key={choice.key} className="flex gap-2"><input type="checkbox" checked={selected.includes(choice.key)} disabled={busy || disabled} onChange={(event) => setSelected((previous) => event.target.checked ? [...previous, choice.key] : previous.filter((key) => key !== choice.key))} />{choice.name}</label>)}
      {cursor && <button type="button" className={button} disabled={busy || disabled} onClick={() => void run(() => loadFiles(active, folder, cursor))}>More files</button>}
      <button type="button" className={button} disabled={busy || disabled || selected.length > 16} onClick={() => void run(async () => { const updated = await api.select(agentId, active.id, { expectedGeneration: active.generation, expectedSelectionRevision: active.selectionRevision, fileKeys: selected }); if (alive.current) { setStatus((current) => current && { ...current, connectors: current.connectors.map((record) => record.id === updated.id ? updated : record) }); setActiveId(null); setChoices([]); } })}>Save selected files</button>
    </div>}
    {status?.configured && <div className="space-y-2 border-t border-line pt-2">
      <label>Account type <select className="field ml-2" value={mode} disabled={busy || disabled} onChange={(event) => setMode(event.target.value as Mode)}><option value="oneDrive">OneDrive</option><option value="sharePoint">SharePoint approved site</option></select></label>
      {mode === 'sharePoint' && <label className="block">Approved site URL <input className="field ml-2" value={site} disabled={busy || disabled} onChange={(event) => setSite(event.target.value)} /></label>}
      <button type="button" className={button} disabled={busy || disabled || (mode === 'sharePoint' && !site.trim())} onClick={() => void run(async () => { const result = await api.connect(agentId, { mode, ...(mode === 'sharePoint' ? { approvedSiteUrl: site.trim() } : {}) }); const safe = validConsent(result.consentUrl); if (!safe) throw new Error('Could not open the Microsoft consent page.'); if (alive.current) setConsent(safe); })}>Connect Microsoft files</button>
      {consent && <a className="block text-accent underline" href={consent} target="_blank" rel="noopener noreferrer">Continue to Microsoft</a>}
      <button type="button" className={button} disabled={busy || disabled} onClick={() => void run(reload)}>Refresh Microsoft files</button>
    </div>}
  </div></ServiceRow>;
}

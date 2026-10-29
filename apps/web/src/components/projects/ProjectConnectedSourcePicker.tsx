import { useEffect, useRef, useState } from 'react';
import type { Project, ProjectsClient, ProjectConnectedSourceChoice } from '@animaos/sdk';
import { connectedProviderName } from './connected-source-presentation';
import { useResetOnChange } from '../../hooks/useResetOnChange';

const NO_AGENTS: readonly { id: string; name: string }[] = [];

export function ProjectConnectedSourcePicker({ project, agentId, agents = NO_AGENTS, api, onSaved, onClose }: {
  project: Project; agentId: string; api: ProjectsClient; onSaved: () => void; onClose: () => void;
  agents?: readonly { id: string; name: string }[];
}) {
  const [query, setQuery] = useState('');
  const [search, setSearch] = useState('');
  const [choices, setChoices] = useState<ProjectConnectedSourceChoice[]>([]);
  const binding = `${agentId}\0${project.id}\0${project.revision}\0${project.accessEpoch}`;
  const [verifiedBinding, setVerifiedBinding] = useState<string | null>(null);
  const [omitted, setOmitted] = useState(0);
  const [selected, setSelected] = useState('');
  const [consent, setConsent] = useState(false);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const requestKey = useRef(crypto.randomUUID());
  const lifetime = useRef<AbortController | null>(null);
  useResetOnChange([binding, search], () => {
    setChoices([]); setSelected(''); setConsent(false); setLoading(true); setError(null); setBusy(false); setOmitted(0);
  });
  useEffect(() => {
    const control = new AbortController(); lifetime.current = control;
    requestKey.current = crypto.randomUUID();
    void api.connectedSourceChoices(project.id, agentId, { query: search, signal: control.signal })
      .then((result) => { if (!control.signal.aborted) { setChoices(result.choices); setVerifiedBinding(binding); setOmitted(result.omittedCount); } })
      .catch((failure: unknown) => { if (!control.signal.aborted) setError(failure instanceof Error ? failure.message : String(failure)); })
      .finally(() => { if (!control.signal.aborted) setLoading(false); });
    return () => control.abort();
  }, [api, project.id, agentId, search, binding]);
  const visibleChoices = verifiedBinding === binding ? choices : [];
  const item = visibleChoices.find((choice) => `${choice.ownerAgentId}:${choice.connectorId}:${choice.citationId}` === selected);
  const share = async () => {
    if (!item || !consent || busy) return;
    const control = lifetime.current; setBusy(true); setError(null);
    try {
      await api.addConnectedSource(project.id, { expectedRevision: project.revision, requestKey: requestKey.current,
        asAgentId: agentId, ownerAgentId: item.ownerAgentId, connectorId: item.connectorId,
        citationId: item.citationId, shareWithProject: true });
      if (control && !control.signal.aborted) onSaved();
    } catch (failure) { if (control && !control.signal.aborted) setError(failure instanceof Error ? failure.message : String(failure)); }
    finally { if (control && !control.signal.aborted) setBusy(false); }
  };
  return <div className="project-picker" role="region" aria-label="Share a connected source">
    <h3>Choose connected content to share</h3>
    <p className="project-note">Choose from accounts owned by this project’s member agents. Connect an account and select its files in Settings first.</p>
    <form onSubmit={(event) => { event.preventDefault(); setSearch(query.trim()); }} className="project-row">
      <label>Find a selected source<input type="search" maxLength={256} value={query} onChange={(event) => setQuery(event.target.value)} /></label>
      <button disabled={loading || busy}>Find sources</button>
    </form>
    {visibleChoices.map((choice) => {
      const key = `${choice.ownerAgentId}:${choice.connectorId}:${choice.citationId}`;
      return <label key={key} className="project-message-choice">
        <input type="radio" name="connected-source" value={key} checked={selected === key} disabled={busy}
          onChange={() => { setSelected(key); setConsent(false); requestKey.current = crypto.randomUUID(); }} />
        <span><strong>{choice.title}</strong><br />{connectedProviderName(choice.provider)} · {choice.accountLabel ?? 'Connected account'} · {agents.find((agent) => agent.id === choice.ownerAgentId)?.name ?? choice.ownerName ?? 'Account owner'}<br />{choice.coverage}</span>
      </label>;
    })}
    {loading && <p role="status">Loading selected account sources…</p>}
    {!loading && !visibleChoices.length && <p>No eligible selected sources. Enable account search and select a file or retain a mail message in the account’s search scope.</p>}
    {omitted > 0 && <p role="status">{omitted} further matches omitted. Refine the source title.</p>}
    <label className="project-check"><input type="checkbox" checked={consent} disabled={!item || busy}
      onChange={(event) => setConsent(event.target.checked)} />Share the selected content with every current project member and use it in new project turns.</label>
    <p className="project-note">The account stays with its owner. Refresh and citations check the original source again. Email coverage may be partial; attachments are omitted.</p>
    {error && <p role="alert">{error}</p>}
    <div className="project-row"><button disabled={!item || !consent || busy || loading || project.status !== 'active'} onClick={() => void share()}>Share selected source</button>
      <button disabled={busy} onClick={onClose}>Cancel</button></div>
  </div>;
}

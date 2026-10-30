import { useState } from 'react';
import type { Project, ProjectsClient } from '@animaos/sdk';
import { useProjectKnowledge } from '../../hooks/useProjectKnowledge';
import { ProjectCitationDialog } from './ProjectCitationDialog';
import { ProjectConnectedSourcePicker } from './ProjectConnectedSourcePicker';
import { connectedProviderName } from './connected-source-presentation';

const NO_AGENTS: readonly { id: string; name: string }[] = [];

export function ProjectKnowledge({ agentId, agents = NO_AGENTS, project, api, invalidate, onChooseFolder, onRemove, onUseInContext }: {
  agentId: string; project: Project; api: ProjectsClient; invalidate: () => void;
  agents?: readonly { id: string; name: string }[];
  onChooseFolder: () => void;
  onRemove: (resourceId: string) => Promise<unknown>;
  onUseInContext: (resourceId: string, selected: boolean) => Promise<unknown>;
}) {
  const folders = project.resources.filter((row) => row.target?.kind === 'knowledge_folder' || row.target?.kind === 'connected_source');
  const knowledge = useProjectKnowledge(agentId, project, folders.length > 0, api, invalidate);
  const [query, setQuery] = useState('');
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [addingConnected, setAddingConnected] = useState(false);
  const active = project.status === 'active';
  const change = async (action: () => Promise<unknown>) => {
    setBusy(true); setError(null);
    try { await action(); } catch (failure) { setError(failure instanceof Error ? failure.message : String(failure)); }
    finally { setBusy(false); }
  };
  return <section className="project-card project-knowledge" aria-label="Project knowledge">
    <div className="project-row"><h2>Search project knowledge</h2>
      <button disabled={!active || busy} onClick={onChooseFolder}>Add folder</button>
      <button disabled={!active || busy} onClick={() => setAddingConnected(true)}>Add connected source</button></div>
    <p className="project-note">Search supported documents and selected connected content shared with this project. Indexing is bounded; coverage and omissions appear below.</p>
    {addingConnected && <ProjectConnectedSourcePicker project={project} agentId={agentId} agents={agents} api={api} onClose={() => setAddingConnected(false)} onSaved={() => { setAddingConnected(false); invalidate(); }} />}
    <form role="search" className="project-knowledge-search" onSubmit={(event) => { event.preventDefault(); void knowledge.search(query); }}>
      <label>Search project knowledge<input type="search" maxLength={512} value={query} placeholder="Find a passage in your project documents"
        onChange={(event) => { setQuery(event.target.value); knowledge.clearSearch(); }} /></label>
      <button disabled={!query.trim() || knowledge.searching || !folders.length}>Search knowledge</button>
    </form>
    {knowledge.searching && <p role="status">Searching project knowledge…</p>}
    {(knowledge.error || error) && <p role="alert">{error ?? knowledge.error}</p>}
    {knowledge.searched && !knowledge.hits.length && <p role="status">No matching passages in the indexed documents.</p>}
    {!!knowledge.hits.length && <ul className="project-knowledge-results" aria-label="Matching passages">{knowledge.hits.map((hit) => <li key={hit.citationId}>
      <button onClick={() => void knowledge.openCitation(hit.citationId)}>{hit.title} · {hit.locator}</button>
      <p>{hit.excerpt}</p>
    </li>)}</ul>}
    {folders.map((resource) => {
      const source = knowledge.status?.sources.find((row) => row.resourceId === resource.id);
      const title = resource.label ?? (resource.target?.kind === 'knowledge_folder' ? resource.target.rootPath : resource.target?.kind === 'connected_source' ? `${connectedProviderName(resource.target.grant.provider)} selected source` : 'Knowledge source');
      return <div key={resource.id} className="project-source">
        <div className="project-row"><strong>{title}</strong><span role="status">{source?.status ?? 'Checking index'}</span>
          <button disabled={busy || knowledge.refreshing || !active} aria-label={`${source?.status === 'partial' || source?.status === 'unavailable' ? 'Retry indexing' : 'Refresh index'} ${title}`}
            onClick={() => void knowledge.refresh(resource.id)}>{source?.status === 'partial' || source?.status === 'unavailable' ? 'Retry indexing' : 'Refresh index'}</button>
          <button disabled={busy} aria-label={`Unlink ${title}`} onClick={() => void change(() => onRemove(resource.id))}>Unlink</button></div>
        {resource.target?.kind === 'knowledge_folder' && <p className="project-folder-path project-note">Read-only · {resource.target.rootPath}</p>}
        {resource.target?.kind === 'connected_source' && <p className="project-note">Read-only · {connectedProviderName(resource.target.grant.provider)} · {agents.find((agent) => agent.id === (resource.target?.kind === 'connected_source' ? resource.target.grant.ownerAgentId : ''))?.name ?? 'Account owner'}{resource.target.grant.sourceKind === 'email_thread' ? ' · bounded conversation; attachments omitted' : ' · one selected file'}</p>}
        {source && <><p>{source.documentCount} {source.documentCount === 1 ? 'document' : 'documents'} indexed · {source.omittedCount} omitted</p>
          {source.indexedAtMs !== null && <p className="project-note">Last indexed {new Date(source.indexedAtMs).toLocaleString()}</p>}
          {source.error && <p role="status">{source.error}</p>}</>}
        <label className="project-check"><input type="checkbox" checked={resource.useInContext} disabled={busy || !active || resource.state !== 'ready'}
          onChange={(event) => void change(() => onUseInContext(resource.id, event.target.checked))} />Use in new project turns</label>
      </div>;
    })}
    {!folders.length && <p>Add a folder or connected source to search its documents here.</p>}
    {(knowledge.opening || knowledge.citation) && <ProjectCitationDialog citation={knowledge.citation} opening={knowledge.opening} onClose={knowledge.closeCitation}
      pageScope={{ agentId, projectId: project.id, api }} />}
  </section>;
}

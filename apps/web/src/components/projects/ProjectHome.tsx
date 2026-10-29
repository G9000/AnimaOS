import { useEffect, useMemo, useRef, useState } from 'react';
import type { Project, ProjectSummary, ProjectLinkTarget } from '@animaos/sdk';
import { useProjects } from '../../hooks/useProjects';
import { daemon } from '../../lib/daemon-api';
import type { Navigate } from '../../lib/hash-route';
import { ProjectSourcePicker } from './ProjectSourcePicker';
import { ProjectKnowledge } from './ProjectKnowledge';
import { ProjectFolderGrant } from './ProjectFolderGrant';
import { useLatest } from '../../hooks/useLatest';
import { useResetOnChange } from '../../hooks/useResetOnChange';
import './projects.css';

type Agent = { id: string; name: string; workspaceRole?: string };
type Tab = 'Chats' | 'Sources' | 'Outputs' | 'Workflows' | 'Members';
type Draft = { title: string; instructions: string; members: string[]; baseRevision: number };
const draftOf = (project: Project): Draft => ({ title: project.title, instructions: project.instructions,
  members: project.members.map((row) => row.agentId), baseRevision: project.revision });
const errorText = (error: unknown) => error instanceof Error ? error.message : String(error);

export function ProjectHome({ agentId, agents, projectId, onSelectAgent, navigate }: {
  agentId: string; agents: readonly Agent[]; projectId: string | null;
  onSelectAgent: (id: string) => void; navigate: Navigate;
}) {
  const view = useProjects(agentId, projectId);
  const [tab, setTab] = useState<Tab>('Chats');
  const [busy, setBusy] = useState(false);
  const [message, setMessage] = useState<string | null>(null);
  const [picker, setPicker] = useState(false);
  const [addingFolder, setAddingFolder] = useState(false);
  // Owner edits survive a conflict or access invalidation, without retaining source payloads.
  const [drafts, setDrafts] = useState(new Map<string, Draft>());
  const storeDraft = (id: string, value: Draft | null) => setDrafts((old) => {
    const next = new Map(old); if (value) next.set(id, value); else next.delete(id); return next;
  });
  const [createTitle, setCreateTitle] = useState('');
  const [createMembers, setCreateMembers] = useState<string[]>([agentId]);
  const createKey = useRef(crypto.randomUUID());
  const [manage, setManage] = useState<ProjectSummary[] | null>(null);
  const scope = `${agentId}\0${projectId ?? ''}`;
  const token = useMemo(() => ({ scope }), [scope]);
  const lifetime = useLatest(token);
  const mounted = useRef(true);
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; }; }, []);
  const isCurrent = () => mounted.current && lifetime.current === token;
  useResetOnChange([scope], () => { setPicker(false); setAddingFolder(false); setMessage(null); setTab('Chats'); setManage(null); setBusy(false); });
  const project = view.project;
  const draftKey = projectId ?? '';
  const draft = drafts.get(draftKey) ?? (project ? draftOf(project) : undefined);
  const edit = (patch: Partial<Draft>) => {
    if (draft) storeDraft(draftKey, { ...draft, ...patch });
  };
  const perform = async (work: (current: () => boolean) => Promise<unknown>, success?: string) => {
    setBusy(true); setMessage(null);
    try { await work(isCurrent); if (isCurrent() && success) setMessage(success); }
    catch (error) { if (isCurrent()) setMessage(errorText(error)); }
    finally { if (isCurrent()) setBusy(false); }
  };
  const availableAgents = agents.filter((agent) => agent.workspaceRole !== 'helper');
  const name = (id: string) => agents.find((agent) => agent.id === id)?.name ?? 'Unavailable agent';
  const openChat = (ownerAgentId: string, sessionId: string) => {
    onSelectAgent(ownerAgentId);
    navigate({ kind: 'session', agentId: ownerAgentId, sessionId });
  };
  const addSource = async (target: ProjectLinkTarget, label: string, useInContext: boolean, requestKey: string) => {
    if (!project) return;
    await view.mutate((api) => api.addResource(project.id, { expectedRevision: project.revision,
      requestKey, asAgentId: agentId, target, role: tab === 'Outputs' ? 'output' : 'reference',
      ...(label.trim() ? { label: label.trim() } : {}), useInContext }));
    if (isCurrent()) setPicker(false);
  };

  return <section className="project-home" aria-label="Projects">
    <div className="project-heading">
      <div><button className="studio-tool-button" onClick={() => navigate({ kind: 'project', projectId: null })}>Projects</button>
        <h1>{project?.title ?? (projectId ? 'Project' : 'A place for shared work')}</h1></div>
      <label>Working as <select aria-label="Working as" value={agentId} onChange={(event) => onSelectAgent(event.target.value)}>
        {availableAgents.map((agent) => <option key={agent.id} value={agent.id}>{agent.name}</option>)}
      </select></label>
    </div>
    {(view.error || message) && <p role="alert" className="project-notice">{message ?? view.error}</p>}
    {addingFolder && <ProjectFolderGrant key={scope} canGrant={Boolean(project && project.status === 'active' && view.live)} onClose={() => setAddingFolder(false)}
      onAdd={async (rootPath, label, requestKey) => {
        if (!project || !view.live) throw new Error('Wait for current project access before granting this folder.');
        await view.mutate((api) => api.addKnowledgeFolder(project.id,
          { expectedRevision: project.revision, asAgentId: agentId, rootPath, requestKey, ...(label ? { label } : {}) }));
        if (isCurrent()) setAddingFolder(false);
      }} />}
    {!projectId ? <>
      <p>Give selected agents a shared brief and selected sources. Each agent keeps its own chats, memory, and permissions.</p>
      <form className="project-card project-create" onSubmit={(event) => { event.preventDefault(); void perform(async (current) => {
        const created = await view.mutate((api) => api.create({ requestKey: createKey.current, title: createTitle, memberAgentIds: createMembers }));
        if (!current()) return;
        createKey.current = crypto.randomUUID(); setCreateTitle('');
        const member = created.members.find((row) => row.agentId === agentId) ?? created.members[0];
        if (member) onSelectAgent(member.agentId);
        navigate({ kind: 'project', projectId: created.id });
      }); }}>
        <label>Project name<input required maxLength={120} value={createTitle} onChange={(event) => {
          setCreateTitle(event.target.value); createKey.current = crypto.randomUUID();
        }} /></label>
        <fieldset><legend>Member agents</legend>{availableAgents.map((agent) => <label key={agent.id} className="project-check">
          <input type="checkbox" checked={createMembers.includes(agent.id)} onChange={(event) => {
            setCreateMembers((old) => event.target.checked ? [...old, agent.id] : old.filter((id) => id !== agent.id));
            createKey.current = crypto.randomUUID();
          }} />{agent.name}</label>)}</fieldset>
        <button disabled={busy || !createTitle.trim() || !createMembers.length}>Create project</button>
      </form>
      <div className="project-grid">{view.projects.map((item) => <button className="project-card" key={item.id}
        onClick={() => navigate({ kind: 'project', projectId: item.id })}>
        <strong>{item.title}</strong><span>{item.chatCount} chats · {item.resourceCount} sources</span>
        <span>{item.members.map((member) => name(member.agentId)).join(', ')}</span>
      </button>)}</div>
      {!view.loading && !view.projects.length && <p>No projects for this agent yet.</p>}
      <button className="studio-tool-button" onClick={() => void perform(async (current) => { const items = await view.api.manage(); if (current()) setManage(items); })}>Manage all projects</button>
      {manage && <div className="project-card"><h2>All projects</h2>{manage.map((item) => <div className="project-row" key={item.id}>
        <span>{item.title} · {item.status} · {item.members.length} members</span>
        {item.members.length ? <button onClick={() => { onSelectAgent(item.members[0].agentId); navigate({ kind: 'project', projectId: item.id }); }}>Open</button>
          : <button disabled={busy} onClick={() => void perform(async (current) => {
            await view.mutate((api) => api.setMembers(item.id, { expectedRevision: item.revision, agentIds: [agentId] }));
            if (!current()) return;
            navigate({ kind: 'project', projectId: item.id });
          })}>Add {name(agentId)} as member</button>}
      </div>)}</div>}
    </> : !project ? <div className="project-card" role="status">
      {view.loading ? 'Loading project…' : !view.live ? 'Reconnecting. Shared sources are hidden until the connection is restored.' : 'This project is unavailable to the selected agent.'}
      <button onClick={view.refresh}>Refresh</button>
    </div> : <>
      <p className="project-members-line">Shared with {project.members.map((member) => name(member.agentId)).join(', ')} · {project.status}</p>
      <div className="project-tabs" role="tablist" aria-label="Project sections">{(['Chats', 'Sources', 'Outputs', 'Workflows', 'Members'] as const).map((item) =>
        <button role="tab" aria-selected={tab === item} key={item} onClick={() => { setTab(item); setPicker(false); setAddingFolder(false); }}>{item}</button>)}</div>
      {tab === 'Chats' && <div className="project-card"><div className="project-row"><h2>Project chats</h2>
        <button disabled={busy || project.status !== 'active'} onClick={() => void perform(async (current) => {
          const member = project.members.find((row) => row.agentId === agentId)!;
          const session = await daemon.createSession(agentId, { title: project.title, projectId: project.id,
            projectRevision: project.revision, memberGeneration: member.generation });
          if (!current()) return;
          openChat(agentId, session.id);
        })}>New chat with {name(agentId)}</button></div>
        {view.chats.map((chat) => <div className="project-row" key={`${chat.ownerAgentId}:${chat.sessionId}`}>
          <button onClick={() => openChat(chat.ownerAgentId, chat.sessionId)}>{chat.title}</button>
          <span>{name(chat.ownerAgentId)}{chat.archived ? ' · archived' : ''}{chat.activeRuns ? ' · working' : ''}</span>
        </div>)}{!view.chats.length && <p>Start a project chat or move an existing chat here from its menu.</p>}
        {view.hasMore && <button disabled={view.loadingMore} onClick={() => void view.loadMore()}>Load more project chats</button>}
        <p className="project-note">Opening a chat switches to the agent that owns it. Only pinned excerpts are shared as context.</p>
      </div>}
      {(tab === 'Sources' || tab === 'Outputs' || tab === 'Workflows') && <>
        {tab === 'Sources' && draft && <form className="project-card" onSubmit={(event) => { event.preventDefault(); void perform(async (current) => {
          await view.mutate((api) => api.update(project.id, { expectedRevision: draft.baseRevision, title: draft.title, instructions: draft.instructions }));
          if (current()) storeDraft(draftKey, null);
        }, 'Project brief saved.'); }}>
          <h2>Shared brief</h2><label>Name<input disabled={busy} required maxLength={120} value={draft.title} onChange={(event) => edit({ title: event.target.value })} /></label>
          <label>Project instructions<textarea disabled={busy} maxLength={4000} rows={5} value={draft.instructions} onChange={(event) => edit({ instructions: event.target.value })} /></label>
          {draft.baseRevision !== project.revision && <p role="status">The project changed. Your draft is kept. Review the latest project before saving.</p>}
          <div className="project-row"><button disabled={busy || draft.baseRevision !== project.revision}>Save brief</button>
            <button type="button" onClick={() => storeDraft(draftKey, { ...draft, baseRevision: project.revision })}>Use latest revision with my draft</button>
            <button type="button" onClick={() => storeDraft(draftKey, null)}>Reset draft</button></div>
        </form>}
        {tab === 'Sources' && <ProjectKnowledge key={`${scope}:${project.revision}:${project.accessEpoch}`} agentId={agentId} agents={agents} project={project} api={view.api} invalidate={view.refresh}
          onChooseFolder={() => setAddingFolder(true)}
          onRemove={(resourceId) => view.mutate((api) => api.removeResource(project.id, resourceId, { expectedRevision: project.revision, asAgentId: agentId }))}
          onUseInContext={(resourceId, useInContext) => view.mutate((api) => api.updateResource(project.id, resourceId, { expectedRevision: project.revision, asAgentId: agentId, useInContext }))} />}
        <div className="project-card"><div className="project-row"><h2>{tab === 'Workflows' ? 'Linked workflows' : 'Selected sources'}</h2>
          <button disabled={busy || project.status !== 'active'} onClick={() => setPicker((old) => !old)}>Link {tab === 'Workflows' ? 'workflow' : 'source'}</button>
          {tab !== 'Workflows' && <button onClick={() => void perform(view.readCoverage)}>Context coverage</button>}</div>
          {picker && <ProjectSourcePicker key={scope} agentId={agentId} project={project} chats={view.chats}
            workflowsOnly={tab === 'Workflows'} onAdd={addSource} onClose={() => setPicker(false)} />}
          {project.resources.filter((resource) => tab === 'Workflows' ? resource.target?.kind === 'workflow' || resource.target?.kind === 'workflow_template'
            : resource.role === (tab === 'Outputs' ? 'output' : 'reference') && resource.target?.kind !== 'knowledge_folder' && resource.target?.kind !== 'connected_source' && resource.target?.kind !== 'workflow' && resource.target?.kind !== 'workflow_template')
            .map((resource) => <div key={resource.id} className="project-source">
              <div className="project-row"><strong>{resource.label ?? (resource.target?.kind === 'workspace_file' ? resource.target.path : resource.state === 'ready' ? 'Selected source' : 'Unavailable source')}</strong>
                <span>{resource.state.replaceAll('_', ' ')}</span>
                <button disabled={busy} onClick={() => void perform(() => view.readPreview(resource.id))}>Preview</button>
                <button disabled={busy} onClick={() => void perform(() => view.mutate((api) => api.removeResource(project.id, resource.id,
                  { expectedRevision: project.revision, asAgentId: agentId })))}>Unlink</button></div>
              <div className="project-row">{([-1, 1] as const).map((direction) => {
                const index = project.resources.findIndex((entry) => entry.id === resource.id);
                const next = index + direction;
                return <button key={direction} disabled={busy || next < 0 || next >= project.resources.length || project.status !== 'active'}
                  onClick={() => void perform(() => {
                    const resourceIds = project.resources.map((entry) => entry.id);
                    [resourceIds[index], resourceIds[next]] = [resourceIds[next], resourceIds[index]];
                    return view.mutate((api) => api.reorderResources(project.id, { expectedRevision: project.revision, asAgentId: agentId, resourceIds }));
                  })}>{direction === -1 ? 'Move earlier' : 'Move later'}</button>;
              })}</div>
              {resource.target && !['workflow', 'workflow_template'].includes(resource.target.kind) && <label className="project-check">
                <input type="checkbox" disabled={busy || resource.state !== 'ready'} checked={resource.useInContext}
                  onChange={(event) => void perform(() => view.mutate((api) => api.updateResource(project.id, resource.id,
                    { expectedRevision: project.revision, asAgentId: agentId, useInContext: event.target.checked })))} />Use in new project turns</label>}
            </div>)}
          {view.preview && <div className="project-preview" role="region" aria-label="Selected source preview">
            {view.preview.workflow ? <><strong>{view.preview.workflow.title}</strong><p>{view.preview.workflow.status} · {name(view.preview.workflow.ownerAgentId)}</p>
              <button onClick={() => { onSelectAgent(view.preview!.workflow!.ownerAgentId); navigate({ kind: 'page', page: 'work' }); }}>Open original agent’s workflows</button></>
              : <pre>{view.preview.text ?? view.preview.state.replaceAll('_', ' ')}</pre>}
          </div>}
          {view.coverage && <div aria-label="Project context coverage"><h3>Current preview · about {view.coverage.estimatedTokens} tokens</h3>
            <p className="project-note">The next turn checks sources again and records what was admitted.</p>
            {view.coverage.resources.map((row) => <p key={row.resourceId}>{project.resources.find((source) => source.id === row.resourceId)?.label ?? 'Selected source'}: {row.state.replaceAll('_', ' ')}{row.shortened ? ' · shortened' : ''}</p>)}</div>}
        </div>
      </>}
      {tab === 'Members' && draft && <div className="project-card"><h2>Member agents</h2>
        <p>Changing members stops active project turns and detaches chats belonging to removed agents. Saved chats, files, results, and workflow records stay with their original owner.</p>
        <fieldset disabled={busy}><legend>Share this project with</legend>{availableAgents.map((agent) => <label key={agent.id} className="project-check">
          <input type="checkbox" checked={draft.members.includes(agent.id)} onChange={(event) => edit({ members: event.target.checked
            ? [...draft.members, agent.id] : draft.members.filter((id) => id !== agent.id) })} />{agent.name}</label>)}</fieldset>
        <button disabled={busy || draft.baseRevision !== project.revision} onClick={() => void perform(async (current) => {
          const result = await view.mutate((api) => api.setMembers(project.id, { expectedRevision: project.revision, agentIds: draft.members }));
          if (!current()) return;
          storeDraft(draftKey, null);
          if (!draft.members.includes(agentId)) { if (draft.members[0]) onSelectAgent(draft.members[0]); else navigate({ kind: 'project', projectId: null }); }
          setMessage(`${result.detachedChats} chats detached. ${result.stopRequestedRunIds.length} turns asked to stop.`);
        })}>Save members</button>
        {draft.baseRevision !== project.revision && <button onClick={() => storeDraft(draftKey, null)}>Reload current members</button>}
        <hr /><p>Archiving or deleting is available when project work is idle.</p>
        <button disabled={busy} onClick={() => void perform(async () => {
          await view.mutate((api) => api.update(project.id, { expectedRevision: project.revision, status: project.status === 'active' ? 'archived' : 'active' }));
        })}>{project.status === 'active' ? 'Archive project' : 'Restore project'}</button>
        <details><summary>Delete project</summary><p>This removes the shared grouping and detaches its chats.</p>
          <button disabled={busy} onClick={() => void perform(async (current) => {
            await view.mutate((api) => api.remove(project.id, { expectedRevision: project.revision }));
            if (!current()) return;
            storeDraft(draftKey, null); navigate({ kind: 'project', projectId: null });
          })}>Delete shared project</button></details>
      </div>}
    </>}
  </section>;
}

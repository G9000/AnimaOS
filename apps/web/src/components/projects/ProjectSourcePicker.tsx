import { useEffect, useRef, useState } from 'react';
import type { ChatResult, Project, ProjectChatLink, ProjectLinkTarget, SessionMessage, WorkflowRun, WorkflowTemplate, WorkspaceFileEntry } from '@animaos/sdk';
import { daemon } from '../../lib/daemon-api';
import { useResetOnChange } from '../../hooks/useResetOnChange';

/** Explicit owner selection; previews here do not grant access until the pin is saved. */
export function ProjectSourcePicker({ agentId, project, chats, workflowsOnly, onAdd, onClose }: {
  agentId: string; project: Project; chats: readonly ProjectChatLink[]; workflowsOnly: boolean;
  onAdd: (target: ProjectLinkTarget, label: string, useInContext: boolean, requestKey: string) => Promise<void>;
  onClose: () => void;
}) {
  const [kind, setKind] = useState<'file' | 'excerpt' | 'result' | 'workflow' | 'workflow_template'>(workflowsOnly ? 'workflow' : 'file');
  const [files, setFiles] = useState<WorkspaceFileEntry[]>([]);
  const [path, setPath] = useState('');
  const [chatKey, setChatKey] = useState('');
  const [messages, setMessages] = useState<SessionMessage[]>([]);
  const [results, setResults] = useState<ChatResult[]>([]);
  const [workflows, setWorkflows] = useState<WorkflowRun[]>([]);
  const [templates, setTemplates] = useState<WorkflowTemplate[]>([]);
  const [selected, setSelected] = useState<string[]>([]);
  const [workflowId, setWorkflowId] = useState('');
  const [generation, setGeneration] = useState(0);
  const [label, setLabel] = useState('');
  const [useInContext, setUseInContext] = useState(false);
  const [busy, setBusy] = useState(false);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const requestKey = useRef(crypto.randomUUID());
  const alive = useRef(true);
  useEffect(() => { alive.current = true; return () => { alive.current = false; }; }, []);
  const chat = chats.find((row) => `${row.ownerAgentId}\0${row.sessionId}` === chatKey);
  const chatOwner = chat?.ownerAgentId;
  const chatSession = chat?.sessionId;
  useResetOnChange([kind, agentId, chatOwner, chatSession], () => {
    setMessages([]); setResults([]); setFiles([]); setWorkflows([]); setTemplates([]); setWorkflowId(''); setSelected([]); setError(null); setLoading(true);
  });
  useEffect(() => {
    const control = new AbortController();
    requestKey.current = crypto.randomUUID();
    void (async () => {
      try {
        if (kind === 'file') {
          const data = await daemon.listWorkspaceFiles();
          if (!control.signal.aborted) { setFiles(data.files); if (data.truncated) setError('Showing a bounded file list. Open Files to locate other files.'); }
        } else if (kind === 'workflow') {
          const data = await daemon.listWorkflows(agentId, { signal: control.signal });
          if (!control.signal.aborted) setWorkflows(data);
        } else if (kind === 'workflow_template') {
          const data = await daemon.listWorkflowTemplates(agentId, { signal: control.signal });
          if (!control.signal.aborted) setTemplates(data);
        } else if (chatOwner && chatSession) {
          const session = await daemon.getSession(chatOwner, chatSession);
          if (control.signal.aborted) return;
          setGeneration(session.clearGeneration ?? 0);
          if (kind === 'excerpt') {
            const page = await daemon.sessionMessages(chatOwner, chatSession, { limit: 40, signal: control.signal });
            if (!control.signal.aborted) setMessages(page.messages.filter((row) => !row.hidden && ['user', 'assistant'].includes(row.role)));
          } else {
            const page = await daemon.chatResults.list(chatOwner, chatSession, { signal: control.signal });
            if (!control.signal.aborted) { setResults(page.results); setGeneration(page.clearGeneration); }
          }
        }
      } catch (failure) { if (!control.signal.aborted) setError(failure instanceof Error ? failure.message : String(failure)); }
      finally { if (!control.signal.aborted) setLoading(false); }
    })();
    return () => control.abort();
  }, [kind, agentId, chatOwner, chatSession]);
  const target: ProjectLinkTarget | null = kind === 'file' && path ? { kind: 'workspace_file', path }
    : kind === 'workflow' && workflowId ? { kind: 'workflow', ownerAgentId: agentId, workflowId }
    : kind === 'workflow_template' && workflowId ? { kind: 'workflow_template', ownerAgentId: agentId, slug: workflowId }
    : chat && selected.length && kind === 'excerpt' ? { kind: 'chat_excerpt', ownerAgentId: chat.ownerAgentId,
      sessionId: chat.sessionId, clearGeneration: generation, messageIds: selected }
    : chat && selected.length && kind === 'result' ? { kind: 'chat_result', ownerAgentId: chat.ownerAgentId,
      sessionId: chat.sessionId, clearGeneration: generation, resultId: selected[0] } : null;
  return <form className="project-picker" aria-label="Link selected source" onSubmit={(event) => {
    event.preventDefault(); if (!target) return; setBusy(true); setError(null);
    void onAdd(target, label, workflowsOnly ? false : useInContext, requestKey.current)
      .catch((failure: unknown) => { if (alive.current) setError(failure instanceof Error ? failure.message : String(failure)); })
      .finally(() => { if (alive.current) setBusy(false); });
  }}>
    <h3>Choose exactly what to share</h3>
    {workflowsOnly && <label>Link type<select value={kind} onChange={(event) => setKind(event.target.value as typeof kind)}>
      <option value="workflow">Workflow</option><option value="workflow_template">Workflow template</option>
    </select></label>}
    {!workflowsOnly && <label>Source type<select value={kind} onChange={(event) => { setKind(event.target.value as typeof kind); setPath(''); setLabel(''); }}>
      <option value="file">Workspace file</option><option value="excerpt">Selected chat messages</option><option value="result">Saved chat result</option>
    </select></label>}
    {kind === 'file' && <label>File<select value={path} onChange={(event) => { setPath(event.target.value); requestKey.current = crypto.randomUUID(); }}>
      <option value="">Choose a file</option>{files.map((file) => <option key={file.path} value={file.path}>{file.path}</option>)}
    </select></label>}
    {(kind === 'excerpt' || kind === 'result') && <>
      <label>Project chat<select value={chatKey} onChange={(event) => setChatKey(event.target.value)}><option value="">Choose a chat</option>
        {chats.map((row) => <option key={`${row.ownerAgentId}\0${row.sessionId}`} value={`${row.ownerAgentId}\0${row.sessionId}`}>{row.title}</option>)}
      </select></label>
      {kind === 'excerpt' && messages.map((row) => <label className="project-message-choice" key={row.id}>
        <input type="checkbox" checked={selected.includes(row.id)} disabled={!selected.includes(row.id) && selected.length >= 4}
          onChange={(event) => { setSelected((old) => event.target.checked ? [...old, row.id] : old.filter((id) => id !== row.id)); requestKey.current = crypto.randomUUID(); }} />
        <span><strong>{row.role === 'user' ? 'You' : 'Agent'}</strong> {row.text.slice(0, 2000)}</span></label>)}
      {kind === 'result' && <label>Result<select value={selected[0] ?? ''} onChange={(event) => {
        setSelected(event.target.value ? [event.target.value] : []); requestKey.current = crypto.randomUUID();
      }}><option value="">Choose a result</option>{results.map((result) => <option key={result.id} value={result.id}>{result.title}</option>)}</select></label>}
      <p className="project-note">Choose up to four exact messages from the latest 40, or one saved result. The full conversation stays private to its agent.</p>
    </>}
    {kind === 'workflow' && <label>Workflow owned by the selected agent<select value={workflowId} onChange={(event) => {
      setWorkflowId(event.target.value); requestKey.current = crypto.randomUUID();
    }}><option value="">Choose a workflow</option>{workflows.map((row) => <option key={row.id} value={row.id}>{row.title} · {row.status}</option>)}</select></label>}
    {kind === 'workflow_template' && <label>Template owned by the selected agent<select value={workflowId} onChange={(event) => {
      setWorkflowId(event.target.value); requestKey.current = crypto.randomUUID();
    }}><option value="">Choose a template</option>{templates.map((row) => <option key={row.slug} value={row.slug}>{row.title}</option>)}</select></label>}
    <label>Label<input maxLength={120} value={label} onChange={(event) => { setLabel(event.target.value); requestKey.current = crypto.randomUUID(); }} /></label>
    {!workflowsOnly && <label className="project-check"><input type="checkbox" checked={useInContext} onChange={(event) => {
      setUseInContext(event.target.checked); requestKey.current = crypto.randomUUID();
    }} />Use in new project turns</label>}
    <p className="project-note">Only the selected source is linked. Context is bounded and checked again for every turn.</p>
    {loading && <p role="status">Loading choices…</p>}{error && <p role="alert">{error}</p>}
    <div className="project-row"><button disabled={busy || loading || !target || project.status !== 'active'}>Link selected source</button>
      <button type="button" onClick={onClose}>Cancel</button></div>
  </form>;
}

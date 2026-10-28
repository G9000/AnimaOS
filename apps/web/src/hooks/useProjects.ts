import { useCallback, useEffect, useLayoutEffect, useRef, useState } from 'react';
import type { AgentEvent, Project, ProjectChatLink, ProjectContextReport,
  ProjectSourcePreview, ProjectSummary, ProjectsClient } from '@animaos/sdk';
import { daemon } from '../lib/daemon-api';
import { useAgentEvents } from './useAgentEvents';
import { useLatest } from './useLatest';
import { useResetOnChange } from './useResetOnChange';

export const PROJECT_POLL_MS = 10_000;

interface ProjectReadState {
  scope: string; list: ProjectSummary[]; project: Project | null; chats: ProjectChatLink[];
  loading: boolean; error: string | null;
  nextCursor?: string | null;
}
const empty = (scope: string): ProjectReadState => ({ scope, list: [], project: null, chats: [], loading: false, error: null });

/** Shared payloads are cleared on every access invalidation and disconnected stream. */
export function useProjects(agentId: string | null, projectId: string | null, visible = true,
  api: ProjectsClient = daemon.projects,
) {
  const scope = `${agentId ?? ''}\0${projectId ?? ''}`;
  const [state, setState] = useState<ProjectReadState>(() => empty(scope));
  const [refreshVersion, setRefreshVersion] = useState(0);
  const [preview, setPreview] = useState<ProjectSourcePreview | null>(null);
  const [coverage, setCoverage] = useState<ProjectContextReport | null>(null);
  const [loadingMore, setLoadingMore] = useState(false);
  const pageController = useRef<AbortController | null>(null);
  const generation = useRef(0);
  const scopeRef = useLatest(scope);
  const controller = useRef<AbortController | null>(null);
  const previewController = useRef<AbortController | null>(null);
  const currentProject = useLatest(state.scope === scope ? state.project : null);

  const invalidate = useCallback(() => {
    generation.current += 1;
    controller.current?.abort();
    previewController.current?.abort();
    pageController.current?.abort(); setLoadingMore(false);
    setPreview(null);
    setCoverage(null);
    setState((old) => ({ ...old, project: null, chats: [], nextCursor: null }));
    setRefreshVersion((value) => value + 1);
  }, []);
  const onEvent = useCallback((event: AgentEvent) => {
    if (event.type === 'stream.snapshot' || event.type === 'stream.resync' || event.type === 'project.updated') invalidate();
  }, [invalidate]);
  const stream = useAgentEvents(agentId, onEvent);
  const live = stream.status === 'open';

  useEffect(() => () => {
    generation.current += 1; controller.current?.abort(); previewController.current?.abort(); pageController.current?.abort();
  }, []);

  useLayoutEffect(() => {
    generation.current += 1;
    controller.current?.abort();
    previewController.current?.abort();
    pageController.current?.abort();
  }, [scope, live]);
  useResetOnChange([scope, live], () => { setLoadingMore(false); setPreview(null); setCoverage(null); setState(empty(scope)); });
  useResetOnChange([scope, live, visible, refreshVersion, api], () => {
    if (agentId && visible) setState((old) => ({ ...old, scope, loading: true, error: null }));
  });

  useEffect(() => {
    if (!agentId || !visible) return;
    const control = new AbortController();
    controller.current = control;
    const request = ++generation.current;
    const matches = () => !control.signal.aborted && generation.current === request && scopeRef.current === scope;
    void (async () => {
      try {
        const list = await api.list(agentId, { signal: control.signal });
        if (!matches()) return;
        // A closed stream can still show owner grouping metadata, never shared payloads.
        if (!projectId || !live) { setState({ scope, list, project: null, chats: [], loading: false, error: null }); return; }
        const project = await api.get(projectId, agentId, { signal: control.signal });
        if (!matches()) return;
        const member = project.members.find((entry) => entry.agentId === agentId);
        if (!member) throw new Error('This agent is no longer a member of this project.');
        const page = await api.chats(projectId, agentId, { limit: 100, signal: control.signal });
        if (!matches()) return;
        if (page.accessEpoch !== project.accessEpoch || page.projectRevision !== project.revision || page.memberGeneration !== member.generation) {
          invalidate(); return;
        }
        setState({ scope, list, project, chats: page.chats, nextCursor: page.nextCursor, loading: false, error: null });
      } catch (error) {
        if (matches()) setState((old) => ({ ...old, scope, project: null, chats: [], loading: false,
          error: error instanceof Error ? error.message : String(error) }));
      }
    })();
    return () => control.abort();
  }, [agentId, projectId, scope, visible, live, refreshVersion, api, invalidate, scopeRef]);

  useEffect(() => {
    if (!visible || !agentId) return;
    const refresh = () => { if (document.visibilityState !== 'hidden') invalidate(); };
    const timer = window.setInterval(refresh, PROJECT_POLL_MS);
    window.addEventListener('focus', refresh);
    window.addEventListener('online', refresh);
    return () => { window.clearInterval(timer); window.removeEventListener('focus', refresh); window.removeEventListener('online', refresh); };
  }, [visible, agentId, invalidate]);

  const readPreview = useCallback(async (resourceId: string) => {
    const project = currentProject.current;
    if (!agentId || !projectId || !live || !project) return;
    previewController.current?.abort();
    const control = new AbortController();
    previewController.current = control;
    const request = generation.current;
    try {
      const result = await api.previewResource(projectId, resourceId, agentId, { signal: control.signal });
      if (control.signal.aborted || request !== generation.current || scopeRef.current !== scope) return;
      const member = project.members.find((entry) => entry.agentId === agentId);
      if (result.accessEpoch !== project.accessEpoch || result.projectRevision !== project.revision || result.memberGeneration !== member?.generation) { invalidate(); return; }
      setPreview(result);
    } catch (error) {
      if (!control.signal.aborted && request === generation.current) setState((old) => ({ ...old, error: error instanceof Error ? error.message : String(error) }));
    }
  }, [agentId, projectId, scope, live, api, invalidate, scopeRef, currentProject]);

  const readCoverage = useCallback(async () => {
    const project = currentProject.current;
    if (!agentId || !projectId || !live || !project) return;
    previewController.current?.abort();
    const control = new AbortController();
    previewController.current = control;
    const request = generation.current;
    const result = await api.context(projectId, agentId, { signal: control.signal });
    if (control.signal.aborted || request !== generation.current || scopeRef.current !== scope) return;
    const member = project.members.find((entry) => entry.agentId === agentId);
    if (result.projectId !== project.id || result.accessEpoch !== project.accessEpoch || result.projectRevision !== project.revision
      || result.memberGeneration !== member?.generation) { invalidate(); return; }
    setCoverage(result);
  }, [agentId, projectId, scope, live, api, invalidate, scopeRef, currentProject]);

  const mutate = useCallback(async <T,>(action: (client: ProjectsClient) => Promise<T>): Promise<T> => {
    const requestScope = scope;
    try {
      const result = await action(api);
      if (scopeRef.current === requestScope) invalidate();
      return result;
    } catch (error) {
      if (scopeRef.current === requestScope) setState((old) => ({ ...old, error: error instanceof Error ? error.message : String(error) }));
      throw error;
    }
  }, [api, scope, invalidate, scopeRef]);

  const loadMore = useCallback(async () => {
    const project = currentProject.current;
    if (!project || !state.nextCursor || loadingMore || !agentId || !projectId || !live) return;
    const control = new AbortController(); pageController.current?.abort(); pageController.current = control;
    const request = generation.current; setLoadingMore(true);
    try {
      const page = await api.chats(projectId, agentId, { cursor: state.nextCursor, limit: 100, signal: control.signal });
      if (control.signal.aborted || request !== generation.current || scopeRef.current !== scope) return;
      if (page.accessEpoch !== project.accessEpoch || page.projectRevision !== project.revision
        || page.memberGeneration !== project.members.find((member) => member.agentId === agentId)?.generation) { invalidate(); return; }
      setState((old) => ({ ...old, chats: [...old.chats, ...page.chats.filter((row) => !old.chats.some((known) => known.ownerAgentId === row.ownerAgentId && known.sessionId === row.sessionId))], nextCursor: page.nextCursor }));
    } catch (error) {
      if (!control.signal.aborted && request === generation.current) setState((old) => ({ ...old, error: error instanceof Error ? error.message : String(error) }));
    } finally { if (!control.signal.aborted && request === generation.current) setLoadingMore(false); }
  }, [agentId, projectId, scope, live, state.nextCursor, loadingMore, api, invalidate, scopeRef, currentProject]);

  const shown = state.scope === scope ? state : empty(scope);
  return { projects: shown.list, project: live ? shown.project : null, chats: live ? shown.chats : [],
    loading: shown.loading, error: shown.error, live, preview: live ? preview : null, coverage: live ? coverage : null,
    refresh: invalidate, readPreview, readCoverage, mutate, api, loadMore, loadingMore, hasMore: live && Boolean(shown.nextCursor) };
}

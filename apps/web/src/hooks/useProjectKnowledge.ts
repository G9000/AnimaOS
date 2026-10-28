import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from 'react';
import type { Project, ProjectsClient, ProjectKnowledgeBinding, ProjectKnowledgeCitation, ProjectKnowledgeHit, ProjectKnowledgeStatus } from '@animaos/sdk';
import { DaemonHttpError } from '@animaos/sdk';
import { useLatest } from './useLatest';
import { useResetOnChange } from './useResetOnChange';

export const KNOWLEDGE_POLL_MS = 2_000;
const REQUEST_KINDS = ['status', 'search', 'citation', 'refresh'] as const;
type RequestKind = typeof REQUEST_KINDS[number];
const errorText = (error: unknown) => error instanceof Error ? error.message : String(error);

/** The parent supplies only its currently authorized, connected project view. */
export function useProjectKnowledge(agentId: string, project: Project | null, visible: boolean, api: ProjectsClient, invalidate: () => void) {
  const member = project?.members.find((row) => row.agentId === agentId);
  const scope = visible && project && member
    ? `${agentId}\0${project.id}\0${project.revision}\0${project.accessEpoch}\0${member.generation}` : '';
  const token = useMemo(() => ({ scope, api }), [scope, api]);
  const latest = useLatest(token);
  const onInvalidate = useLatest(invalidate);
  const [status, setStatus] = useState<ProjectKnowledgeStatus | null>(null);
  const [hits, setHits] = useState<ProjectKnowledgeHit[]>([]);
  const [citation, setCitation] = useState<ProjectKnowledgeCitation | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [searching, setSearching] = useState(false);
  const [opening, setOpening] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [searched, setSearched] = useState(false);
  const controllers = useRef<Partial<Record<RequestKind, AbortController>>>({});
  const indexFingerprint = useRef<string | null>(null);
  const operationEpoch = useRef(0);
  const publishedGenerations = useRef(new Map<string, number>());

  const abort = useCallback((kind?: RequestKind) => {
    for (const key of kind ? [kind] : REQUEST_KINDS) controllers.current[key]?.abort();
  }, []);
  useLayoutEffect(() => {
    indexFingerprint.current = null;
    operationEpoch.current = 0; publishedGenerations.current.clear();
    return () => abort();
  }, [token, abort]);
  useResetOnChange([token], () => {
    setStatus(null); setHits([]); setCitation(null); setError(null); setSearching(false); setOpening(false); setRefreshing(false); setSearched(false);
  });

  const clearReads = useCallback(() => {
    abort('search'); abort('citation'); setHits([]); setCitation(null); setSearching(false); setOpening(false); setSearched(false);
  }, [abort]);
  const closeCitation = useCallback(() => { abort('citation'); setCitation(null); setOpening(false); }, [abort]);
  const failed = useCallback((failure: unknown) => {
    clearReads(); setError(errorText(failure));
    if (failure instanceof DaemonHttpError && (failure.status === 403 || failure.status === 404 || failure.status === 409)) {
      abort(); setStatus(null); setRefreshing(false); onInvalidate.current();
    }
  }, [clearReads, abort, onInvalidate]);
  const start = useCallback((kind: RequestKind) => {
    abort(kind);
    const control = new AbortController(); controllers.current[kind] = control;
    return { control, current: () => !control.signal.aborted && latest.current === token && Boolean(scope) };
  }, [abort, latest, token, scope]);
  const startOperation = useCallback((kind: 'search' | 'refresh') => {
    // Explicit actions fence earlier polls and actions. Background progress
    // cannot obsolete an in-flight action's acknowledgement.
    abort('status');
    const epoch = ++operationEpoch.current;
    const request = start(kind);
    return { ...request, publishable: () => request.current() && operationEpoch.current === epoch };
  }, [abort, start]);
  const accepts = useCallback((binding: ProjectKnowledgeBinding) => {
    if (project && member && binding.projectRevision === project.revision && binding.accessEpoch === project.accessEpoch && binding.memberGeneration === member.generation) return true;
    abort(); clearReads(); setStatus(null); setError('Project access changed. Refreshing the current project.'); onInvalidate.current(); return false;
  }, [project, member, abort, clearReads, onInvalidate]);
  const publishStatus = useCallback((value: ProjectKnowledgeStatus, fromSearch = false) => {
    // Within one authorized lifetime a delayed response cannot roll a source
    // back. A full scope reset clears this fence, including after a restart.
    if (value.sources.some((row) => row.generation < (publishedGenerations.current.get(row.resourceId) ?? 0))) return false;
    // Progress labels do not change admitted evidence. A new generation, removed
    // source, or unavailable capability still fences every pending/cached read.
    const fingerprint = JSON.stringify(value.sources.map((row) => [row.resourceId, row.generation, row.status === 'unavailable']));
    if (indexFingerprint.current !== null && indexFingerprint.current !== fingerprint) {
      if (fromSearch) closeCitation(); else clearReads();
    }
    for (const row of value.sources) publishedGenerations.current.set(row.resourceId, row.generation);
    indexFingerprint.current = fingerprint; setStatus(value); return true;
  }, [clearReads, closeCitation]);
  const readStatus = useCallback(async () => {
    if (!scope || !project) return;
    const request = start('status');
    try {
      const value = await api.knowledgeStatus(project.id, agentId, { signal: request.control.signal });
      if (request.current() && accepts(value) && publishStatus(value)) setError(null);
    } catch (failure) {
      if (request.current()) { setStatus(null); failed(failure); }
    }
  }, [scope, project, start, api, agentId, accepts, publishStatus, failed]);
  // Use primitives so unrelated parent renders do not restart polling or pending reads.
  const readStatusLatest = useLatest(readStatus);
  useEffect(() => { if (scope) void readStatusLatest.current(); }, [scope, api, readStatusLatest]);
  const indexing = status?.sources.some((row) => row.status === 'indexing') ?? false;
  useEffect(() => {
    if (!scope || !indexing) return;
    const timer = window.setInterval(() => { if (document.visibilityState !== 'hidden') void readStatusLatest.current(); }, KNOWLEDGE_POLL_MS);
    return () => window.clearInterval(timer);
  }, [scope, indexing, readStatusLatest]);

  const search = useCallback(async (query: string) => {
    clearReads(); setError(null);
    if (!scope || !project || !query.trim()) return;
    if (query.length > 512) { setError('Search is limited to 512 characters.'); return; }
    const request = startOperation('search'); setSearching(true);
    try {
      const value = await api.searchKnowledge(project.id, agentId, { query: query.trim(), limit: 8, signal: request.control.signal });
      if (!request.publishable() || !accepts(value)) return;
      if (!publishStatus(value, true)) return;
      // Polls started while the search ran must not replace its accepted snapshot.
      abort('status'); setHits(value.hits); setSearched(true);
    } catch (failure) { if (request.publishable()) failed(failure); }
    finally { if (request.current()) setSearching(false); }
  }, [scope, project, clearReads, startOperation, api, agentId, accepts, publishStatus, failed, abort]);
  const openCitation = useCallback(async (citationId: string) => {
    closeCitation(); setError(null);
    if (!scope || !project) return;
    const request = start('citation'); setOpening(true);
    try {
      const value = await api.openKnowledgeCitation(project.id, citationId, agentId, { signal: request.control.signal });
      if (request.current() && accepts(value)) setCitation(value);
    } catch (failure) { if (request.current()) failed(failure); }
    finally { if (request.current()) setOpening(false); }
  }, [scope, project, closeCitation, start, api, agentId, accepts, failed]);
  const refresh = useCallback(async (resourceId?: string) => {
    clearReads(); setError(null);
    if (!scope || !project) return;
    const request = startOperation('refresh'); setRefreshing(true);
    try {
      const value = await api.refreshKnowledge(project.id, { asAgentId: agentId, ...(resourceId ? { resourceId } : {}) }, { signal: request.control.signal });
      if (request.publishable() && accepts(value) && publishStatus(value)) abort('status');
    } catch (failure) { if (request.publishable()) failed(failure); }
    finally { if (request.current()) setRefreshing(false); }
  }, [clearReads, scope, project, startOperation, api, agentId, accepts, publishStatus, failed, abort]);
  return { status: scope ? status : null, hits: scope ? hits : [], citation: scope ? citation : null,
    error, searching, opening, refreshing, searched, search, openCitation, closeCitation, refresh, clearSearch: clearReads };
}

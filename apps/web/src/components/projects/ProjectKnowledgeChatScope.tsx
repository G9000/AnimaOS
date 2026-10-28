import { useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState, type ReactNode } from 'react';
import type { Project, ProjectsClient } from '@animaos/sdk';
import { ProjectKnowledgeScope } from './ProjectKnowledgeLink';
import { useProjects } from '../../hooks/useProjects';
import { useProjectKnowledge } from '../../hooks/useProjectKnowledge';
import { useLatest } from '../../hooks/useLatest';
import { useResetOnChange } from '../../hooks/useResetOnChange';
import { ProjectCitationDialog } from './ProjectCitationDialog';

/** The binding comes from the current session, never from message text. */
export function ProjectKnowledgeChatScope({ agentId, projectId, sessionId, clearGeneration, bindingRevision, children }: {
  agentId: string; projectId: string | null; sessionId: string; clearGeneration: number; bindingRevision: number; children: ReactNode;
}) {
  const [citationId, setCitationId] = useState<string | null>(null);
  useResetOnChange([agentId, projectId, sessionId, clearGeneration, bindingRevision], () => setCitationId(null));
  const close = useCallback(() => setCitationId(null), []);
  const value = useMemo(() => projectId ? { agentId, projectId, onOpen: setCitationId } : null, [agentId, projectId]);
  return <ProjectKnowledgeScope.Provider value={value}>{children}
    {citationId && projectId && <ProjectChatCitation key={citationId} agentId={agentId} projectId={projectId} citationId={citationId} onClose={close} />}
  </ProjectKnowledgeScope.Provider>;
}

function ProjectChatCitation({ agentId, projectId, citationId, onClose }: {
  agentId: string; projectId: string; citationId: string; onClose: () => void;
}) {
  const view = useProjects(agentId, projectId);
  const admitted = useRef(false);
  useLayoutEffect(() => {
    if (view.project) admitted.current = true;
    else if (admitted.current) onClose();
  }, [view.project, onClose]);
  if (!view.project) return <ProjectCitationDialog citation={null} opening={view.loading && view.live}
    error={view.error ?? (!view.live ? 'Reconnect to verify current project access.' : view.loading ? null : 'This project is unavailable to the selected agent.')} onClose={onClose} />;
  return <VerifiedCitation key={`${view.project.revision}:${view.project.accessEpoch}`} agentId={agentId} project={view.project}
    api={view.api} invalidate={view.refresh} citationId={citationId} onClose={onClose} />;
}

function VerifiedCitation({ agentId, project, api, invalidate, citationId, onClose }: {
  agentId: string; project: Project; api: ProjectsClient; invalidate: () => void; citationId: string; onClose: () => void;
}) {
  const knowledge = useProjectKnowledge(agentId, project, true, api, invalidate);
  const open = useLatest(knowledge.openCitation);
  const indexVersion = knowledge.status ? JSON.stringify(knowledge.status.sources.map((row) => [row.resourceId, row.generation])) : null;
  useEffect(() => {
    // Reopen the same opaque identity after publication. The SDK endpoint checks
    // the source/hash again; it cannot substitute a changed document's passage.
    if (indexVersion !== null) void open.current(citationId);
  }, [citationId, indexVersion, open]);
  return <ProjectCitationDialog citation={knowledge.citation} opening={knowledge.opening || (indexVersion === null && !knowledge.error)}
    error={knowledge.error ?? (indexVersion !== null && !knowledge.opening && !knowledge.citation ? 'This citation is no longer current. Close it and open the source again.' : null)} onClose={onClose} />;
}

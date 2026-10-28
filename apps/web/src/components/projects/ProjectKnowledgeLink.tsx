import { createContext, useContext, type ReactNode } from 'react';

export const ProjectKnowledgeScope = createContext<{
  agentId: string; projectId: string; onOpen: (citationId: string) => void;
} | null>(null);

export function isKnowledgeCitationLink(href: string | undefined): boolean {
  return Boolean(href && /\/api\/projects\/[^?#]*\/knowledge\/citations(?:\/|[?#]|$)/.test(href));
}

/** Parse identities only. Fetching always uses the typed SDK and current UI scope. */
export function knowledgeCitationTarget(href: string): { projectId: string; citationId: string; agentId: string } | null {
  try {
    const url = new URL(href, window.location.origin);
    if (url.origin !== window.location.origin || url.hash || url.username || url.password) return null;
    const match = /^\/api\/projects\/([^/]+)\/knowledge\/citations\/([^/]+)$/.exec(url.pathname);
    if (!match || Array.from(url.searchParams.keys()).join() !== 'asAgentId') return null;
    const projectId = decodeURIComponent(match[1]); const citationId = decodeURIComponent(match[2]);
    const agentId = url.searchParams.get('asAgentId') ?? '';
    if (![projectId, citationId, agentId].every((id) => /^[A-Za-z0-9_:-]{1,200}$/.test(id))) return null;
    return { projectId, citationId, agentId };
  } catch { return null; }
}

export function ProjectKnowledgeLink({ href, children }: { href: string; children: ReactNode }) {
  const scope = useContext(ProjectKnowledgeScope);
  const target = knowledgeCitationTarget(href);
  if (!scope || !target || scope.projectId !== target.projectId || scope.agentId !== target.agentId) return <span>{children}</span>;
  return <button type="button" className="text-accent underline underline-offset-2" onClick={() => scope.onOpen(target.citationId)}>{children}</button>;
}

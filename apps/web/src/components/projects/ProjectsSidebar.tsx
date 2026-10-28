import { useProjects } from '../../hooks/useProjects';
import type { Navigate } from '../../lib/hash-route';

/** Grouping metadata only; opening a project performs fresh member reads. */
export function ProjectsSidebar({ agentId, navigate }: { agentId: string; navigate: Navigate }) {
  const view = useProjects(agentId, null);
  return <nav aria-label="Projects" className="px-3 py-2">
    <div className="project-row"><strong>Projects</strong><button type="button" className="studio-tool-button"
      onClick={() => navigate({ kind: 'project', projectId: null })}>New project</button></div>
    {view.projects.map((project) => <button key={project.id} type="button" className="session-row w-full text-left"
      onClick={() => navigate({ kind: 'project', projectId: project.id })}>{project.title}</button>)}
    {view.error && <p role="status">Project list unavailable.</p>}
  </nav>;
}

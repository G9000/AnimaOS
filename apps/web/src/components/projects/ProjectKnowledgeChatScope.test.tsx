import { act, fireEvent, render, screen, waitFor } from '@testing-library/react';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';
import type { Project, ProjectsClient } from '@animaos/sdk';
import { ProjectKnowledgeChatScope } from './ProjectKnowledgeChatScope';
import { ProjectKnowledgeLink } from './ProjectKnowledgeLink';
import { KNOWLEDGE_POLL_MS } from '../../hooks/useProjectKnowledge';

const fixtures = vi.hoisted(() => ({ view: {} as Record<string, unknown> }));
vi.mock('../../hooks/useProjects', () => ({ useProjects: () => fixtures.view }));
const project: Project = { id: 'project_one', title: 'Research', revision: 4, accessEpoch: 2, status: 'active',
  members: [{ agentId: 'a', generation: 3, joinedAtMs: 1 }], resources: [], instructions: '',
  createdAtMs: 1, updatedAtMs: 1, resourceCount: 1, chatCount: 0 };
const binding = { projectRevision: 4, accessEpoch: 2, memberGeneration: 3 };
const hit = { citationId: 'c', resourceId: 'r', documentId: 'd', title: 'Annual plan.pdf', locator: 'Page 12', excerpt: 'Evidence excerpt' };
function chat(projectId = 'project_one') {
  return <ProjectKnowledgeChatScope agentId="a" projectId={projectId} sessionId="s" clearGeneration={0} bindingRevision={1}>
    <ProjectKnowledgeLink href="/api/projects/project_one/knowledge/citations/c?asAgentId=a">Open passage</ProjectKnowledgeLink>
  </ProjectKnowledgeChatScope>;
}
describe('project citation in a chat', () => {
  afterEach(() => vi.useRealTimers());
  beforeEach(() => {
    fixtures.view = { project, live: true, error: null, refresh: vi.fn(), api: {
      knowledgeStatus: vi.fn().mockResolvedValue({ ...binding, sources: [] }),
      openKnowledgeCitation: vi.fn().mockResolvedValue({ ...binding, ...hit, text: 'Verified current passage' }),
    } as unknown as ProjectsClient };
  });
  it('checks the current project before reading the citation, then clears it on invalidation', async () => {
    const view = render(chat());
    const open = (fixtures.view.api as ProjectsClient).openKnowledgeCitation;
    expect(open).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Open passage' }));
    expect(await screen.findByRole('dialog', { name: 'Annual plan.pdf' })).toHaveTextContent('Verified current passage');
    expect(open).toHaveBeenCalledWith('project_one', 'c', 'a', { signal: expect.any(AbortSignal) });
    fixtures.view.project = null;
    view.rerender(chat());
    expect(screen.queryByText('Verified current passage')).not.toBeInTheDocument();
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });
  it('aborts a pending passage when the conversation project changes', async () => {
    let resolve!: (value: unknown) => void;
    const open = vi.fn().mockImplementation(() => new Promise((done) => { resolve = done; }));
    (fixtures.view.api as ProjectsClient).openKnowledgeCitation = open;
    const view = render(chat()); fireEvent.click(screen.getByRole('button', { name: 'Open passage' }));
    await waitFor(() => expect(open).toHaveBeenCalledTimes(1));
    const signal = open.mock.calls[0][3].signal as AbortSignal;
    view.rerender(chat('project_other'));
    expect(signal.aborted).toBe(true);
    await act(async () => resolve({ ...binding, ...hit, text: 'Late private passage' }));
    expect(screen.queryByText('Late private passage')).not.toBeInTheDocument();
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });

  it('keeps a verified citation open across a status-only indexing completion', async () => {
    vi.useFakeTimers();
    const source = { resourceId: 'r', status: 'indexing', documentCount: 1, indexedAtMs: null, generation: 1, omittedCount: 0 };
    const api = fixtures.view.api as ProjectsClient;
    api.knowledgeStatus = vi.fn().mockResolvedValueOnce({ ...binding, sources: [source] })
      .mockResolvedValue({ ...binding, sources: [{ ...source, status: 'ready' }] });
    render(chat());
    await act(async () => { fireEvent.click(screen.getByRole('button', { name: 'Open passage' })); });
    expect(screen.getByRole('dialog', { name: 'Annual plan.pdf' })).toHaveTextContent('Verified current passage');
    await act(async () => { await vi.advanceTimersByTimeAsync(KNOWLEDGE_POLL_MS); });
    expect(screen.getByRole('dialog', { name: 'Annual plan.pdf' })).toHaveTextContent('Verified current passage');
    expect(api.openKnowledgeCitation).toHaveBeenCalledTimes(1);
  });

  it('clears old text and verifies the same citation again after an index generation changes', async () => {
    vi.useFakeTimers();
    const source = { resourceId: 'r', status: 'indexing', documentCount: 1, indexedAtMs: null, generation: 0, omittedCount: 0 };
    const api = fixtures.view.api as ProjectsClient;
    api.knowledgeStatus = vi.fn().mockResolvedValueOnce({ ...binding, sources: [source] })
      .mockResolvedValue({ ...binding, sources: [{ ...source, status: 'ready', generation: 1 }] });
    let resolve!: (value: unknown) => void;
    api.openKnowledgeCitation = vi.fn().mockResolvedValueOnce({ ...binding, ...hit, text: 'Verified current passage' })
      .mockImplementationOnce(() => new Promise((done) => { resolve = done; }));
    render(chat());
    await act(async () => { fireEvent.click(screen.getByRole('button', { name: 'Open passage' })); });
    expect(screen.getByText('Verified current passage')).toBeVisible();
    await act(async () => { await vi.advanceTimersByTimeAsync(KNOWLEDGE_POLL_MS); });
    expect(screen.queryByText('Verified current passage')).not.toBeInTheDocument();
    expect(api.openKnowledgeCitation).toHaveBeenCalledTimes(2);
    await act(async () => resolve({ ...binding, ...hit, text: 'Verified current passage' }));
    expect(screen.getByRole('dialog', { name: 'Annual plan.pdf' })).toHaveTextContent('Verified current passage');
  });
});

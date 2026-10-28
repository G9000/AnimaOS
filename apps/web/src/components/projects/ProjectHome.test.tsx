import { act, fireEvent, render, screen, waitFor, within } from '@testing-library/react';
import { beforeEach, describe, expect, it, vi } from 'vitest';
import type { Project } from '@animaos/sdk';
import { ProjectHome } from './ProjectHome';

const fixtures = vi.hoisted(() => ({ view: {} as Record<string, unknown>, createSession: vi.fn(), pickFolder: vi.fn() }));
vi.mock('../../hooks/useProjects', () => ({ useProjects: () => fixtures.view }));
vi.mock('../../lib/daemon-api', () => ({ daemon: { createSession: fixtures.createSession, pickKnowledgeFolder: fixtures.pickFolder } }));
vi.mock('./ProjectSourcePicker', () => ({ ProjectSourcePicker: () => <div>Source picker</div> }));
const project: Project = { id: 'project_shared', title: 'House research', status: 'active', revision: 4, accessEpoch: 1,
  members: [{ agentId: 'a', generation: 1, joinedAtMs: 1 }, { agentId: 'b', generation: 1, joinedAtMs: 1 }],
  instructions: 'Current owner brief', resources: [], createdAtMs: 1, updatedAtMs: 2, chatCount: 1, resourceCount: 0 };
const agents = [{ id: 'a', name: 'Alice' }, { id: 'b', name: 'Bob' }];
function setup(agentId = 'a') {
  const navigate = vi.fn(); const select = vi.fn();
  return { navigate, select, ...render(<ProjectHome agentId={agentId} agents={agents} projectId={project.id} onSelectAgent={select} navigate={navigate} />) };
}
describe('shared project home', () => {
  beforeEach(() => {
    fixtures.createSession.mockReset();
    fixtures.pickFolder.mockReset();
    fixtures.view = { project, projects: [project], chats: [{ ownerAgentId: 'b', sessionId: 'chat:b', title: 'Bob’s research', archived: false, activeRuns: 0 }],
      loading: false, error: null, live: true, preview: null, coverage: null, refresh: vi.fn(), readCoverage: vi.fn(), readPreview: vi.fn(),
      api: { update: vi.fn().mockRejectedValue(new Error('Project changed. Refresh and retry.')),
        knowledgeStatus: vi.fn().mockResolvedValue({ projectRevision: 4, accessEpoch: 1, memberGeneration: 1, sources: [] }),
        addKnowledgeFolder: vi.fn().mockResolvedValue(project) },
      mutate: vi.fn((action: (api: unknown) => Promise<unknown>) => action((fixtures.view as { api: unknown }).api)) };
  });
  it('opens a member chat with its original owner and keeps the shared project identity on agent selection', () => {
    const { select, navigate } = setup();
    fireEvent.change(screen.getByLabelText('Working as'), { target: { value: 'b' } });
    expect(select).toHaveBeenLastCalledWith('b'); expect(navigate).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Bob’s research' }));
    expect(navigate).toHaveBeenCalledWith({ kind: 'session', agentId: 'b', sessionId: 'chat:b' });
  });
  it('keeps an edited brief after a conflict and requires review against the latest revision', async () => {
    const view = setup();
    fireEvent.click(screen.getByRole('tab', { name: 'Sources' }));
    fireEvent.change(screen.getByLabelText('Project instructions'), { target: { value: 'My unsaved changes' } });
    fireEvent.click(screen.getByRole('button', { name: 'Save brief' }));
    await waitFor(() => expect(screen.getByRole('alert')).toHaveTextContent('Project changed'));
    fixtures.view.project = { ...project, revision: 5, instructions: 'Another owner update' };
    view.rerender(<ProjectHome agentId="a" agents={agents} projectId={project.id} onSelectAgent={view.select} navigate={view.navigate} />);
    expect(screen.getByLabelText('Project instructions')).toHaveValue('My unsaved changes');
    expect(screen.getByRole('button', { name: 'Save brief' })).toBeDisabled();
  });
  it('hides cached source payloads when project access is invalidated', () => {
    fixtures.view.preview = { text: 'Selected donor payload' };
    const view = setup(); fireEvent.click(screen.getByRole('tab', { name: 'Sources' }));
    expect(screen.getByText('Selected donor payload')).toBeVisible();
    fixtures.view.project = null; fixtures.view.live = false;
    view.rerender(<ProjectHome agentId="a" agents={agents} projectId={project.id} onSelectAgent={view.select} navigate={view.navigate} />);
    expect(screen.queryByText('Selected donor payload')).not.toBeInTheDocument();
    expect(screen.getByRole('status')).toHaveTextContent('Reconnecting');
  });
  it('does not navigate to a late new chat after switching member agents', async () => {
    let resolve!: (value: { id: string }) => void;
    fixtures.createSession.mockImplementation(() => new Promise((done) => { resolve = done; }));
    const view = setup(); fireEvent.click(screen.getByRole('button', { name: 'New chat with Alice' }));
    view.rerender(<ProjectHome agentId="b" agents={agents} projectId={project.id} onSelectAgent={view.select} navigate={view.navigate} />);
    await act(async () => resolve({ id: 'chat:late-a' }));
    expect(view.navigate).not.toHaveBeenCalled();
  });

  it('requires explicit read-only consent after choosing a folder outside the workspace', async () => {
    fixtures.pickFolder.mockResolvedValue({ rootPath: 'D:\\Research' });
    setup(); fireEvent.click(screen.getByRole('tab', { name: 'Sources' }));
    fireEvent.click(screen.getByRole('button', { name: 'Add folder' }));
    fireEvent.click(screen.getByRole('button', { name: 'Choose folder' }));
    await screen.findByText('D:\\Research');
    const add = screen.getByRole('button', { name: 'Grant read-only access and add folder' });
    expect(add).toBeDisabled();
    expect(screen.getByText(/This folder and its supported documents/)).toBeVisible();
    fireEvent.click(screen.getByRole('checkbox', { name: /I grant this project read-only access/ }));
    fireEvent.click(add);
    await waitFor(() => expect((fixtures.view.api as { addKnowledgeFolder: ReturnType<typeof vi.fn> }).addKnowledgeFolder)
      .toHaveBeenCalledWith(project.id, expect.objectContaining({ expectedRevision: 4, asAgentId: 'a', rootPath: 'D:\\Research', requestKey: expect.any(String) })));
  });
  it('shows searchable folder coverage and opens only a verified bounded citation', async () => {
    fixtures.view.project = { ...project, resourceCount: 1, resources: [{ id: 'folder', role: 'reference', useInContext: true,
      label: 'Research', state: 'ready', createdAtMs: 1, target: { kind: 'knowledge_folder', rootPath: 'D:\\Research', rootIdentity: 'root' } }] };
    const binding = { projectRevision: 4, accessEpoch: 1, memberGeneration: 1 };
    const status = { ...binding, sources: [{ resourceId: 'folder', status: 'partial', documentCount: 3, indexedAtMs: 1, generation: 1,
      omittedCount: 2, error: 'Two files could not be read' }] };
    const hit = { citationId: 'citation', resourceId: 'folder', documentId: 'doc', title: 'Annual plan.pdf', locator: 'Page 12', excerpt: 'A matching passage' };
    Object.assign(fixtures.view.api as object, { knowledgeStatus: vi.fn().mockResolvedValue(status),
      searchKnowledge: vi.fn().mockResolvedValue({ ...status, hits: [hit] }),
      openKnowledgeCitation: vi.fn().mockResolvedValue({ ...binding, ...hit, text: 'Verified page twelve passage' }), refreshKnowledge: vi.fn().mockResolvedValue(status) });
    setup(); fireEvent.click(screen.getByRole('tab', { name: 'Sources' }));
    await screen.findByText('3 documents indexed · 2 omitted');
    expect(screen.getByText('Two files could not be read')).toBeVisible();
    expect(screen.getByRole('button', { name: 'Retry indexing Research' })).toBeEnabled();
    fireEvent.change(screen.getByRole('searchbox', { name: 'Search project knowledge' }), { target: { value: 'annual plan' } });
    fireEvent.click(screen.getByRole('button', { name: 'Search knowledge' }));
    fireEvent.click(await screen.findByRole('button', { name: 'Annual plan.pdf · Page 12' }));
    expect(await screen.findByRole('dialog', { name: 'Annual plan.pdf' })).toHaveTextContent('Verified page twelve passage');
    expect(screen.getByRole('button', { name: 'Close citation' })).toHaveFocus();
    fireEvent.keyDown(screen.getByRole('dialog'), { key: 'Escape' });
    expect(screen.queryByRole('dialog')).not.toBeInTheDocument();
  });
  it('preserves the owner folder draft through a same-project authorization refresh', async () => {
    fixtures.pickFolder.mockResolvedValue({ rootPath: 'D:\\Research' });
    const view = setup(); fireEvent.click(screen.getByRole('tab', { name: 'Sources' }));
    fireEvent.click(screen.getByRole('button', { name: 'Add folder' }));
    fireEvent.click(screen.getByRole('button', { name: 'Choose folder' }));
    await screen.findByText('D:\\Research');
    fireEvent.click(screen.getByRole('checkbox', { name: /I grant this project read-only access/ }));
    fixtures.view.project = null;
    view.rerender(<ProjectHome agentId="a" agents={agents} projectId={project.id} onSelectAgent={view.select} navigate={view.navigate} />);
    expect(screen.getByText('D:\\Research')).toBeVisible();
    expect(screen.getByRole('button', { name: 'Grant read-only access and add folder' })).toBeDisabled();
    fixtures.view.project = { ...project, revision: 5 };
    view.rerender(<ProjectHome agentId="a" agents={agents} projectId={project.id} onSelectAgent={view.select} navigate={view.navigate} />);
    expect(screen.getByRole('checkbox', { name: /I grant this project read-only access/ })).toBeChecked();
    expect(screen.getByRole('button', { name: 'Grant read-only access and add folder' })).toBeEnabled();
    view.rerender(<ProjectHome agentId="b" agents={agents} projectId={project.id} onSelectAgent={view.select} navigate={view.navigate} />);
    expect(screen.queryByText('D:\\Research')).not.toBeInTheDocument();
  });
  it('uses the existing resource controls for folder context selection and revocation', async () => {
    const update = vi.fn().mockResolvedValue(project); const remove = vi.fn().mockResolvedValue(project);
    Object.assign(fixtures.view.api as object, { updateResource: update, removeResource: remove });
    fixtures.view.project = { ...project, resourceCount: 1, resources: [{ id: 'folder', role: 'reference', useInContext: true,
      label: 'Research', state: 'ready', createdAtMs: 1, target: { kind: 'knowledge_folder', rootPath: 'D:\\Research', rootIdentity: 'root' } }] };
    setup(); fireEvent.click(screen.getByRole('tab', { name: 'Sources' }));
    const knowledge = within(screen.getByLabelText('Project knowledge'));
    fireEvent.click(knowledge.getByRole('checkbox', { name: 'Use in new project turns' }));
    await waitFor(() => expect(update).toHaveBeenCalledWith(project.id, 'folder', { expectedRevision: 4, asAgentId: 'a', useInContext: false }));
    fireEvent.click(knowledge.getByRole('button', { name: 'Unlink Research' }));
    await waitFor(() => expect(remove).toHaveBeenCalledWith(project.id, 'folder', { expectedRevision: 4, asAgentId: 'a' }));
  });
});

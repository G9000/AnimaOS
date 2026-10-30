import { act, fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it, vi } from 'vitest';
import type { Project, ProjectsClient } from '@animaos/sdk';
import { ProjectKnowledge } from './ProjectKnowledge';

const project: Project = { id: 'p', title: 'Research', revision: 4, accessEpoch: 2, status: 'active',
  members: [{ agentId: 'a', generation: 3, joinedAtMs: 1 }], instructions: '',
  resources: [{ id: 'r', target: { kind: 'knowledge_folder', rootPath: 'D:\\Research', rootIdentity: 'root' },
    state: 'ready', role: 'reference', label: 'Research folder', useInContext: true, createdAtMs: 1 }],
  createdAtMs: 1, updatedAtMs: 1, resourceCount: 1, chatCount: 0 };
const binding = { projectRevision: 4, accessEpoch: 2, memberGeneration: 3 };
const source = { resourceId: 'r', status: 'ready', documentCount: 2, indexedAtMs: null, generation: 1, omittedCount: 1 };
const hit = { citationId: 'c', resourceId: 'r', documentId: 'd', title: 'Research.pdf', locator: 'Page 8', excerpt: 'Evidence excerpt' };
const hybrid = { mode: 'hybrid', semanticState: 'ready', provider: 'fastembed', model: 'local-model',
  indexedWindows: 4, eligibleWindows: 4, omittedWindows: 0 };

function show(retrieval?: object, hits = [hit]) {
  const api = { knowledgeStatus: vi.fn().mockResolvedValue({ ...binding, sources: [source] }),
    searchKnowledge: vi.fn().mockResolvedValue({ ...binding, sources: [source], hits, ...(retrieval ? { retrieval } : {}) }),
    refreshKnowledge: vi.fn().mockResolvedValue({ ...binding, sources: [source] }) };
  render(<ProjectKnowledge project={project} agentId="a" api={api as unknown as ProjectsClient} invalidate={vi.fn()}
    onChooseFolder={vi.fn()} onRemove={vi.fn()} onUseInContext={vi.fn()} />);
  return api;
}
function search() {
  fireEvent.change(screen.getByRole('searchbox'), { target: { value: 'evidence' } });
  fireEvent.click(screen.getByRole('button', { name: 'Search knowledge' }));
}

describe('project knowledge search mode', () => {
  it('shows meaning search only after an accepted hybrid result and clears it when the query changes', async () => {
    show(hybrid);
    expect(screen.queryByText(/Meaning and keyword search/)).not.toBeInTheDocument();
    search();
    expect(await screen.findByText('Meaning and keyword search')).toBeVisible();
    expect(screen.getByText('2 documents indexed · 1 omitted')).toBeVisible();
    expect(screen.getByText('ready')).toBeVisible();
    fireEvent.change(screen.getByRole('searchbox'), { target: { value: 'changed' } });
    expect(screen.queryByText(/Meaning and keyword search/)).not.toBeInTheDocument();
  });

  it('reports partial meaning coverage and omissions without changing source indexing status', async () => {
    show({ ...hybrid, semanticState: 'partial', indexedWindows: 3, eligibleWindows: 5, omittedWindows: 2 });
    search();
    expect(await screen.findByText(/Meaning and keyword search/)).toHaveTextContent('Meaning search coverage: 3 of 5 passage windows indexed; 2 omitted.');
    expect(screen.getByText('ready')).toBeVisible();
  });

  it('shows keyword fallback and its owner-readable reason even when no passages match', async () => {
    show({ mode: 'keyword', semanticState: 'unavailable', indexedWindows: 0, eligibleWindows: 5,
      omittedWindows: 5, reason: 'Local meaning search is unavailable.' }, []);
    search();
    expect(await screen.findByText(/Keyword search only/)).toHaveTextContent('Local meaning search is unavailable.');
    expect(screen.queryByText(/Meaning and keyword search/)).not.toBeInTheDocument();
    expect(screen.getByText('No matching passages in the indexed documents.')).toBeVisible();
  });

  it('uses the actual keyword mode even when a local model and indexed coverage are reported', async () => {
    show({ ...hybrid, mode: 'keyword', semanticState: 'partial', indexedWindows: 3, eligibleWindows: 5, omittedWindows: 2 });
    search();
    expect(await screen.findByText(/Keyword search only/)).toHaveTextContent('3 of 5 passage windows indexed; 2 omitted.');
    expect(screen.queryByText(/Meaning and keyword search/)).not.toBeInTheDocument();
  });

  it('makes no search-mode claim for older daemon responses', async () => {
    show(); search();
    expect(await screen.findByText('Evidence excerpt')).toBeVisible();
    expect(screen.queryByText(/Meaning and keyword search|Keyword search only/)).not.toBeInTheDocument();
  });

  it('clears the search-mode claim when refreshing a source', async () => {
    show(hybrid); search();
    expect(await screen.findByText('Meaning and keyword search')).toBeVisible();
    await act(async () => { fireEvent.click(screen.getByRole('button', { name: 'Refresh index Research folder' })); });
    expect(screen.queryByText(/Meaning and keyword search/)).not.toBeInTheDocument();
  });
});

import { act, fireEvent, render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';

import { approvalFixture } from '../../test/live';
import { ApprovalCard } from './ApprovalCard';
import { daemon } from '../../lib/daemon-api';

describe('ApprovalCard', () => {
  const imageArgs = { _anima_exact_image: true, prompt: '<img src=x> A blue bird', sourceAttachmentId: null, sourceName: null, sourceSha256: null, provider: 'openai', model: 'gpt-image-2.5-sunburst', size: '1024x1024', quality: 'medium', count: 1, disclosure: 'OpenAI API charges apply.' };
  it('offers an exact single-use image request with plain prompt and transfer disclosure', async () => {
    const request = approvalFixture('image', { tool: 'generate_image', arguments: JSON.stringify(imageArgs), expiresAtMs: 4_000_000_000_000 });
    const onDecide = vi.fn().mockResolvedValue(null);
    const { container } = render(<ApprovalCard approval={request} onDecide={onDecide} canPersist />);
    expect(screen.getByLabelText('Image prompt')).toHaveTextContent('<img src=x> A blue bird');
    expect(container.querySelector('img')).toBeNull();
    expect(screen.getByText(/sent to OpenAI/)).toHaveTextContent('API charges apply');
    expect(screen.queryByRole('button', { name: 'Always allow' })).toBeNull();
    expect(screen.queryByRole('button', { name: 'Allow for this session' })).toBeNull();
    await userEvent.click(screen.getByRole('button', { name: 'Allow once and generate' }));
    expect(onDecide).toHaveBeenCalledWith(request, { decision: 'allow_once' });
  });
  it.each([
    { arguments: '{}' },
    { arguments: JSON.stringify({ ...imageArgs, sourceAttachmentId: 'source' }) },
    { arguments: JSON.stringify({ ...imageArgs, extra: true }) },
    { arguments: JSON.stringify(imageArgs), argumentsTruncated: true },
  ])('blocks incomplete image approval without reusable grants', (change) => {
    render(<ApprovalCard approval={approvalFixture('image', { tool: 'generate_image', ...change })} onDecide={vi.fn()} canPersist />);
    expect(screen.getByRole('alert')).toHaveTextContent('incomplete');
    expect(screen.getByRole('button', { name: 'Allow once and generate' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Deny' })).toBeEnabled();
    expect(screen.queryByRole('button', { name: 'Always allow' })).toBeNull();
  });
  it('previews the exact edit source through authenticated bytes and denies a failed preview', async () => {
    let finish!: (blob: Blob) => void;
    const download = vi.spyOn(daemon, 'downloadAttachment').mockImplementation(() => new Promise((resolve) => { finish = resolve; }));
    const create = vi.spyOn(URL, 'createObjectURL').mockReturnValue('blob:source');
    const revoke = vi.spyOn(URL, 'revokeObjectURL').mockImplementation(() => {});
    const request = approvalFixture('edit-image', { tool: 'edit_image', arguments: JSON.stringify({ ...imageArgs, sourceAttachmentId: 'source-id', sourceName: 'Original.png', sourceSha256: 'a'.repeat(64) }), expiresAtMs: 4_000_000_000_000 });
    const onDecide = vi.fn().mockResolvedValue(null);
    const { unmount } = render(<ApprovalCard approval={request} onDecide={onDecide} />);
    expect(screen.getByRole('button', { name: 'Allow once and edit' })).toBeDisabled();
    expect(download).toHaveBeenCalledWith(request.agentId, 'source-id', { signal: expect.any(AbortSignal) });
    const sourceReview = screen.getByLabelText('Selected input image');
    expect(sourceReview).toHaveTextContent('Original.png');
    expect(sourceReview).not.toHaveTextContent('source-id');
    expect(sourceReview).not.toHaveTextContent('a'.repeat(64));
    await act(async () => finish(new Blob(['png'], { type: 'image/png' })));
    const image = screen.getByRole('img', { name: 'Original.png' });
    expect(create).toHaveBeenCalledOnce();
    expect(screen.getByRole('button', { name: 'Allow once and edit' })).toBeEnabled();
    fireEvent.error(image);
    expect(screen.getByRole('button', { name: 'Allow once and edit' })).toBeDisabled();
    await userEvent.click(screen.getByRole('button', { name: 'Deny' }));
    expect(onDecide).toHaveBeenCalledWith(request, { decision: 'deny' });
    unmount();
    expect(revoke).toHaveBeenCalledWith('blob:source');
    download.mockRestore(); create.mockRestore(); revoke.mockRestore();
  });
  it('does not fetch image bytes for a read-only approval', () => {
    const download = vi.spyOn(daemon, 'downloadAttachment');
    const request = approvalFixture('edit-image', { tool: 'edit_image', arguments: JSON.stringify({ ...imageArgs, sourceAttachmentId: 'source-id', sourceName: 'Original.png', sourceSha256: 'a'.repeat(64) }) });
    render(<ApprovalCard approval={request} />);
    expect(download).not.toHaveBeenCalled();
    expect(screen.getByText('Image approval is available to the owner.')).toBeVisible();
    download.mockRestore();
  });
  it('never falls back to reusable approval for malformed document requests', async () => {
    const onDecide = vi.fn().mockResolvedValue(null);
    render(<ApprovalCard approval={approvalFixture('document', { tool: 'apply_document_edit', arguments: '{}', expiresAtMs: 4_000_000_000_000 })} onDecide={onDecide} canPersist />);
    expect(await screen.findByRole('alert')).toHaveTextContent('incomplete');
    expect(screen.getByRole('button', { name: 'Apply these changes' })).toBeDisabled();
    expect(screen.queryByRole('button', { name: 'Always allow' })).toBeNull();
    expect(screen.queryByRole('button', { name: 'Allow for this session' })).toBeNull();
  });

  it('does not fetch private document reviews when the card is read only', async () => {
    const load = vi.spyOn(daemon.documentEdits, 'review');
    try {
      render(<ApprovalCard approval={approvalFixture('document', { tool: 'apply_document_edit', arguments: '{}' })} />);
      expect(await screen.findByText('Document review is available to the owner.')).toBeVisible();
      expect(load).not.toHaveBeenCalled();
    } finally { load.mockRestore(); }
  });

  it('loads the exact owner review through the approval card before allowing a decision', async () => {
    const hash = 'a'.repeat(64);
    const source = 'b'.repeat(64);
    const args = { _anima_exact_document_edit: true, proposal_id: 'p1', review_sha256: hash, candidate_sha256: 'c'.repeat(64), source_sha256: source, file_path: 'report.docx', expected_destination: { state: 'absent' } };
    const inventory = { version: 1 as const, format: 'docx' as const, targets: [], retained_hyperlinks: [], limitations: [] };
    const load = vi.spyOn(globalThis, 'fetch').mockResolvedValue(Response.json({ version: 1, proposal_id: 'p1', review_sha256: hash, source: { kind: 'attachment', display_name: 'source.docx' }, source_sha256: source, file_path: 'report.docx', expected_destination: { state: 'absent' }, expires_at_ms: 4_000_000_000_000, preview_quality: 'structural_content', before: inventory, after: inventory, changes: [{ operation_index: 0, label: 'Text', before: 'old', after: 'new', derived: false }], preservation: { unchanged_parts: 2, edited_parts: 1, unchanged_bytes_verified: true, container_normalized: true } }));
    const onDecide = vi.fn().mockResolvedValue(null);
    const request = approvalFixture('document', { tool: 'apply_document_edit', arguments: JSON.stringify(args), expiresAtMs: 4_000_000_000_000 });
    try {
      render(<ApprovalCard approval={request} onDecide={onDecide} />);
      expect(await screen.findByText('After: new')).toBeVisible();
      expect(load).toHaveBeenCalledWith(`/api/document-edits/p1?agent_id=${encodeURIComponent(request.agentId)}&session_id=${encodeURIComponent(request.sessionId)}`, expect.objectContaining({ signal: expect.any(AbortSignal), credentials: 'same-origin', redirect: 'error' }));
      await userEvent.click(screen.getByRole('button', { name: 'Apply these changes' }));
      expect(onDecide).toHaveBeenCalledWith(request, { decision: 'allow_once' });
    } finally { load.mockRestore(); }
  });
  it('shows the call’s arguments as text, never as markup', () => {
    const approval = approvalFixture('apr_1', {
      arguments: '{"command":"<img src=x onerror=alert(1)> **bold**"}',
      argumentsTruncated: true,
    });
    const { container } = render(
      <ApprovalCard canPersist approval={approval} />,
    );

    const card = screen.getByRole('region', { name: 'Approval needed: bash' });
    expect(within(card).getByLabelText('Arguments')).toHaveTextContent(
      '{"command":"<img src=x onerror=alert(1)> **bold**"}',
    );
    expect(container.querySelector('img')).toBeNull();
    expect(container.querySelector('strong')?.textContent).toBe('bash');
    expect(
      within(card).getByText('Arguments shortened to 16 KiB.'),
    ).toBeVisible();
    expect(within(card).getByText('Runs commands')).toBeVisible();
    expect(within(card).getByLabelText('Note (optional)')).toHaveAttribute(
      'maxLength',
      '1000',
    );
    for (const name of [
      'Allow once',
      'Allow for this session',
      'Always allow',
      'Deny',
    ])
      expect(within(card).getByRole('button', { name })).toBeDisabled();
  });

  it('makes hidden characters in the arguments visible', () => {
    const approval = approvalFixture('apr_1', {
      arguments: '{"command":"ls\u202E"}',
    });
    render(<ApprovalCard approval={approval} />);

    expect(screen.getByLabelText('Arguments')).toHaveTextContent(
      '{"command":"ls\\u{202e}"}',
    );
  });

  it('makes hidden characters in the tool name and the scope visible', () => {
    const approval = approvalFixture('apr_1', {
      tool: 'ba\u202Esh',
      suggestedMatcher: { kind: 'command_prefix', value: 'git\u200B status' },
    });
    render(<ApprovalCard approval={approval} />);

    const card = screen.getByRole('region');
    expect(card.querySelector('.approval-card-tool')).toHaveTextContent(
      'ba\\u{202e}sh',
    );
    expect(card.querySelector('.approval-card-scope')).toHaveTextContent(
      'ba\\u{202e}sh commands starting with “git\\u{200b} status”',
    );
  });

  it('sends a trimmed note with a denial and no matcher', async () => {
    const user = userEvent.setup();
    const approval = approvalFixture('apr_1');
    const onDecide = vi.fn().mockResolvedValue(null);
    render(<ApprovalCard approval={approval} onDecide={onDecide} />);

    await user.type(screen.getByLabelText('Note (optional)'), '  not now  ');
    await user.click(screen.getByRole('button', { name: 'Deny' }));

    expect(onDecide).toHaveBeenCalledWith(approval, {
      decision: 'deny',
      note: 'not now',
    });
    expect(
      await screen.findByText('Denied. The companion carries on without it.'),
    ).toBeVisible();
    expect(
      screen.queryByRole('button', { name: 'Allow once' }),
    ).not.toBeInTheDocument();
  });

  it('sends the suggested scope, or the edited one, with the scoped decisions', async () => {
    const user = userEvent.setup();
    const approval = approvalFixture('apr_1');
    const onDecide = vi.fn().mockResolvedValue(null);
    const { unmount } = render(
      <ApprovalCard canPersist approval={approval} onDecide={onDecide} />,
    );
    expect(
      screen.getByText(/bash commands starting with “git status”/),
    ).toBeVisible();

    await user.click(screen.getByRole('button', { name: 'Edit' }));
    const value = screen.getByLabelText('Match value');
    await user.clear(value);
    await user.type(value, 'git');
    await user.click(screen.getByRole('button', { name: 'Always allow' }));
    expect(onDecide).toHaveBeenLastCalledWith(approval, {
      decision: 'allow_always',
      matcher: { kind: 'command_prefix', value: 'git' },
    });
    unmount();

    render(<ApprovalCard approval={approval} onDecide={onDecide} />);
    await user.click(screen.getByRole('button', { name: 'Edit' }));
    await user.selectOptions(
      screen.getByLabelText('Match by'),
      'Any call of this tool',
    );
    expect(screen.queryByLabelText('Match value')).not.toBeInTheDocument();
    await user.click(
      screen.getByRole('button', { name: 'Allow for this session' }),
    );
    expect(onDecide).toHaveBeenLastCalledWith(approval, {
      decision: 'allow_session',
      matcher: { kind: 'any', value: '' },
    });
  });

  it('shows why a decision did not go through and lets the owner try again', async () => {
    const user = userEvent.setup();
    const approval = approvalFixture('apr_1');
    const onDecide = vi
      .fn()
      .mockResolvedValueOnce('This approval was already resolved')
      .mockResolvedValueOnce(null);
    render(<ApprovalCard approval={approval} onDecide={onDecide} />);

    await user.click(screen.getByRole('button', { name: 'Allow once' }));
    expect(await screen.findByRole('alert')).toHaveTextContent(
      'This approval was already resolved',
    );
    await user.click(screen.getByRole('button', { name: 'Allow once' }));
    expect(await screen.findByText('Allowed once. Continuing…')).toBeVisible();
    expect(screen.queryByRole('alert')).not.toBeInTheDocument();
  });

  it('warns beside the scoped buttons when the scope is broad', async () => {
    const user = userEvent.setup();
    const warning =
      'A rule this broad lets your companion run almost anything, including changing its own approval settings.';
    render(
      <ApprovalCard
        canPersist
        approval={approvalFixture('apr_1', {
          suggestedMatcher: { kind: 'command_prefix', value: 'ls -la' },
        })}
        onDecide={vi.fn()}
      />,
    );
    expect(screen.queryByText(warning)).not.toBeInTheDocument();

    await user.click(screen.getByRole('button', { name: 'Edit' }));
    await user.selectOptions(
      screen.getByLabelText('Match by'),
      'Any call of this tool',
    );
    expect(screen.getByText(warning)).toBeVisible();
  });

  it('keeps cut arguments and an empty scope from being saved as a rule', async () => {
    const user = userEvent.setup();
    const onDecide = vi.fn().mockResolvedValue(null);
    const { unmount } = render(
      <ApprovalCard
        canPersist
        approval={approvalFixture('apr_1', { argumentsTruncated: true })}
        onDecide={onDecide}
      />,
    );
    expect(
      screen.getByText('Arguments were cut; only Allow once or Deny.'),
    ).toBeVisible();
    expect(
      screen.getByRole('button', { name: 'Allow for this session' }),
    ).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Always allow' })).toBeDisabled();
    expect(screen.getByRole('button', { name: 'Allow once' })).toBeEnabled();
    unmount();

    render(
      <ApprovalCard
        canPersist
        approval={approvalFixture('apr_2', {
          suggestedMatcher: { kind: 'command_prefix', value: '' },
        })}
        onDecide={onDecide}
      />,
    );
    expect(screen.queryByText(/starting with “”/)).not.toBeInTheDocument();
    expect(screen.getByRole('button', { name: 'Always allow' })).toBeDisabled();
    await user.click(screen.getByRole('button', { name: 'Edit' }));
    await user.type(screen.getByLabelText('Match value'), 'ls');
    expect(screen.getByRole('button', { name: 'Always allow' })).toBeEnabled();
  });

  it('offers no rule for another agent’s approval', () => {
    render(
      <ApprovalCard
        approval={approvalFixture('apr_1', { agentId: 'helper-7' })}
        onDecide={vi.fn()}
      />,
    );

    expect(
      screen.queryByRole('button', { name: 'Always allow' }),
    ).not.toBeInTheDocument();
    expect(
      screen.getByText('Rules for other agents are not managed here yet.'),
    ).toBeVisible();
    for (const name of ['Allow once', 'Allow for this session', 'Deny'])
      expect(screen.getByRole('button', { name })).toBeEnabled();
  });
});

describe('ApprovalCard for a coding agent', () => {
  it('shows what the coding agent wants as labelled rows and keeps the full request one click away', () => {
    const approval = approvalFixture('apr_acp', {
      tool: 'acp_edit',
      class: 'write',
      arguments: JSON.stringify({
        agent: 'Claude Agent',
        title: 'Edit sum.js',
        file_path: '/Users/ana/work/calc/sum.js',
        locations: ['/Users/ana/work/calc/sum.js'],
        sessionId: 'acp-1',
      }),
    });
    render(<ApprovalCard canPersist approval={approval} />);
    const card = screen.getByRole('region', { name: 'Approval needed: acp_edit' });
    expect(within(card).getByText('Claude Agent wants to edit sum.js. It waits until you answer.')).toBeVisible();
    expect(within(card).getByText('File')).toBeVisible();
    expect(within(card).getByText('~/work/calc/sum.js')).toBeVisible();
    expect(within(card).getByText('acp-1')).toBeVisible();
    const raw = within(card).getByText('Full request').closest('details');
    expect(raw).not.toHaveAttribute('open');
    expect(within(card).getByLabelText('Arguments')).toHaveTextContent('"file_path"');
  });
});

describe('ApprovalCard reason', () => {
  it('says why a dangerous command asks, and nothing for other approvals', () => {
    const { rerender } = render(
      <ApprovalCard
        approval={approvalFixture('apr_1', { reason: 'dangerous_command' })}
      />,
    );
    expect(
      screen.getByText('Asked because this command is hard to undo.'),
    ).toBeVisible();
    rerender(<ApprovalCard approval={approvalFixture('apr_2')} />);
    expect(
      screen.queryByText('Asked because this command is hard to undo.'),
    ).toBeNull();
  });

  it('names the specific risk when the floor gave one', () => {
    render(
      <ApprovalCard
        approval={approvalFixture('apr_1', {
          reason: 'dangerous_command',
          reasonDetail: 'runs hidden (encoded) commands',
        })}
      />,
    );
    expect(
      screen.getByText('Asked because this command runs hidden (encoded) commands.'),
    ).toBeVisible();
    expect(
      screen.queryByText('Asked because this command is hard to undo.'),
    ).toBeNull();
  });
});

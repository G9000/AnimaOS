import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { SkillEditor } from './SkillEditor';
import { SkillDraftCard } from './SkillDraftCard';
import { SkillSupportingFiles } from './SkillSupportingFiles';
import { skillDraftFixture } from '../../test/skills';

describe('skill package review', () => {
  it('shows the pinned GitHub source and excluded metadata before approval', () => {
    const draft = {
      ...skillDraftFixture('skd_github'),
      source: 'import' as const,
      githubSource: {
        repository: 'example/notes',
        commit: '1234567890abcdef1234567890abcdef12345678',
        path: 'skills/notes',
        sourceUrl: 'https://github.com/example/notes/tree/1234567890abcdef1234567890abcdef12345678/skills/notes',
        packageHash: 'a'.repeat(64),
        ignoredPaths: ['.gitignore'],
      },
    };
    render(<SkillDraftCard draft={draft} onApprove={vi.fn()} onReject={vi.fn()} />);
    expect(screen.getByRole('link', { name: 'example/notes/skills/notes' })).toHaveAttribute('href', draft.githubSource.sourceUrl);
    expect(screen.getByText(/1234567890ab/)).toBeVisible();
    expect(screen.getByText(/\.gitignore/)).toBeVisible();
    expect(screen.getByText(/Imported files remain inactive until you keep this skill/)).toBeVisible();
  });

  it('adds supporting text files to the complete owner save', async () => {
    const save = vi.fn().mockResolvedValue(true);
    render(
      <SkillEditor
        initial={{
          slug: 'notes',
          name: 'Notes',
          description: 'Take notes',
          body: 'Use template',
        }}
        saveLabel="Save skill"
        onSave={save}
        onCancel={vi.fn()}
      />,
    );
    await userEvent.click(
      screen.getByRole('button', { name: 'Add supporting file' }),
    );
    await userEvent.type(
      screen.getByLabelText('File path 1'),
      'templates/note.md',
    );
    await userEvent.type(
      screen.getByLabelText('File content 1'),
      '<img src=x onerror=alert(1)>',
    );
    await userEvent.click(screen.getByRole('button', { name: 'Save skill' }));
    expect(save).toHaveBeenCalledWith(
      expect.objectContaining({
        files: [
          {
            path: 'templates/note.md',
            content: '<img src=x onerror=alert(1)>',
          },
        ],
      }),
    );
  });

  it('shows every proposed file as inert text before approval', async () => {
    const approve = vi.fn().mockResolvedValue(true);
    const draft = {
      ...skillDraftFixture('skd_pkg'),
      files: [
        { path: 'scripts/example.sh', content: 'echo inert' },
        { path: 'templates/note.md', content: '<img src=x onerror=alert(1)>' },
      ],
    };
    const { container } = render(
      <SkillDraftCard draft={draft} onApprove={approve} onReject={vi.fn()} />,
    );
    // The proposal folds its steps and files under "See steps".
    await userEvent.click(screen.getByText('See steps'));
    const script = screen.getByText(/^scripts\/example\.sh \(/);
    expect(script).toBeVisible();
    await userEvent.click(script);
    await userEvent.click(screen.getByText(/^templates\/note\.md \(/));
    expect(screen.getByText('echo inert')).toBeVisible();
    expect(screen.getByText('<img src=x onerror=alert(1)>')).toBeVisible();
    expect(container.querySelector('img')).toBeNull();
    await userEvent.click(screen.getByRole('button', { name: 'Keep' }));
    expect(approve).toHaveBeenCalledWith(draft, {});
  });

  it('shows binary asset metadata without embedding its bytes in the review', async () => {
    const binary = { path: 'assets/image.png', content: 'AAH+/w==', encoding: 'base64' as const };
    const draft = skillDraftFixture('skd_binary', { files: [binary] });
    const approve = vi.fn().mockResolvedValue(true);
    const { container } = render(<SkillDraftCard draft={draft} onApprove={approve} onReject={vi.fn()} />);
    await userEvent.click(screen.getByText('See steps'));
    expect(screen.getByText('assets/image.png (4 bytes, binary asset)')).toBeVisible();
    await userEvent.click(screen.getByText('assets/image.png (4 bytes, binary asset)'));
    expect(screen.getByText(/Binary asset preserved in the package/)).toBeVisible();
    expect(container.innerHTML).not.toContain(binary.content);
    expect(container.querySelector('img, audio, video, iframe, object')).toBeNull();
    await userEvent.click(screen.getByRole('button', { name: 'Keep' }));
    expect(approve).toHaveBeenCalledWith(draft, {});
  });

  it('preserves binary bytes and root setup files while the owner edits instructions', async () => {
    const save = vi.fn().mockResolvedValue(true);
    const files = [
      { path: 'assets/image.png', content: 'AAH+/w==', encoding: 'base64' as const },
      { path: 'requirements.txt', content: 'httpx==0.28.1\n' },
      { path: '.skillignore', content: 'tests/\n' },
    ];
    const { container } = render(<SkillEditor initial={{ slug: 'research', name: 'Research', description: 'Research topics', body: 'Original instructions', files }} saveLabel="Save skill" onSave={save} onCancel={vi.fn()} />);
    expect(screen.queryByLabelText('File content 1')).toBeNull();
    expect(screen.getByLabelText('File content 2')).toHaveValue('httpx==0.28.1\n');
    expect(container.innerHTML).not.toContain(files[0].content);
    await userEvent.clear(screen.getByLabelText('Instructions'));
    await userEvent.type(screen.getByLabelText('Instructions'), 'Reviewed instructions');
    await userEvent.click(screen.getByRole('button', { name: 'Save skill' }));
    expect(screen.queryByRole('alert')).toBeNull();
    expect(save).toHaveBeenCalledWith(expect.objectContaining({ body: 'Reviewed instructions', files }));
  });

  it('allows a package with more than 32 files to remain complete on save', async () => {
    const save = vi.fn().mockResolvedValue(true);
    const files = Array.from({ length: 40 }, (_, index) => ({ path: `engine/module-${index}.py`, content: `value = ${index}\n` }));
    render(<SkillEditor initial={{ slug: 'engine', name: 'Engine', description: 'Run the engine', body: 'Use the installed engine', files }} saveLabel="Save skill" onSave={save} onCancel={vi.fn()} />);
    await userEvent.click(screen.getByRole('button', { name: 'Save skill' }));
    expect(save).toHaveBeenCalledWith(expect.objectContaining({ files }));
  });

  it('bounds large text previews while keeping the full package file intact', () => {
    const content = `${'a'.repeat(128 * 1024)}END_OF_COMPLETE_FILE`;
    const { container } = render(<SkillSupportingFiles files={[{ path: 'references/large.md', content }]} />);
    expect(screen.getByLabelText('File preview: references/large.md')).toHaveTextContent('Preview limited to 128 KiB. The complete file is preserved.');
    expect(container.textContent).not.toContain('END_OF_COMPLETE_FILE');
  });
});

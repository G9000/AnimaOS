import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { SkillEditor } from './SkillEditor';
import { SkillDraftCard } from './SkillDraftCard';
import { skillDraftFixture } from '../../test/skills';

describe('skill package review', () => {
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
    expect(screen.getByText('scripts/example.sh')).toBeVisible();
    expect(screen.getByText('echo inert')).toBeVisible();
    expect(screen.getByText('<img src=x onerror=alert(1)>')).toBeVisible();
    expect(container.querySelector('img')).toBeNull();
    await userEvent.click(screen.getByRole('button', { name: 'Keep' }));
    expect(approve).toHaveBeenCalledWith(draft, {});
  });
});

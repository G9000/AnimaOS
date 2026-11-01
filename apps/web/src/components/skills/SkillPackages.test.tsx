import { render, screen, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { SkillEditor } from './SkillEditor';
import { SkillDraftCard } from './SkillDraftCard';
import { SkillSupportingFiles } from './SkillSupportingFiles';
import { skillDraftFixture } from '../../test/skills';

function urlSourceFixture() {
  return {
    requestedUrl: 'https://skills.example.org/research/SKILL.md',
    sourceUrl: 'https://skills.example.org/research/SKILL.md',
    observedAtMs: Date.UTC(2026, 10, 1, 3, 4, 5),
    packageHash: 'b'.repeat(64),
    originalSkillText: '\uFEFF---\r\nname: Research\r\ncustom: retained\r\n---\r\n<img src=x onerror=alert(1)>\r\n',
    coverage: 'referenced_files' as const,
    files: [
      {
        path: 'SKILL.md',
        sourceUrl: 'https://skills.example.org/research/SKILL.md',
        sha256: 'c'.repeat(64),
        sizeBytes: 123,
      },
      {
        path: 'references/guide.md',
        sourceUrl: 'https://skills.example.org/research/references/guide.md',
        sha256: 'd'.repeat(64),
        sizeBytes: 45,
      },
    ],
  };
}

describe('skill package review', () => {
  it('shows the pinned GitHub source and excluded metadata before approval', async () => {
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
        originalSkillText: '---\nname: Notes\ncustom: retained\n---\nOriginal instructions',
      },
    };
    render(<SkillDraftCard draft={draft} onApprove={vi.fn()} onReject={vi.fn()} />);
    expect(screen.getByRole('link', { name: 'example/notes/skills/notes' })).toHaveAttribute('href', draft.githubSource.sourceUrl);
    expect(screen.getByText(/1234567890ab/)).toBeVisible();
    expect(screen.getByText(/\.gitignore/)).toBeVisible();
    expect(screen.getByText(/Imported files remain inactive until you keep this skill/)).toBeVisible();
    await userEvent.click(screen.getByText('Original package instructions and metadata'));
    expect(screen.getByText(/custom: retained/)).toBeVisible();
    expect(screen.queryByText(/referenced or selected files/)).toBeNull();
  });

  it('reviews a URL capture, its original formatting and file hashes before Keep', async () => {
    const approve = vi.fn().mockResolvedValue(true);
    const urlSource = urlSourceFixture();
    const draft = { ...skillDraftFixture('skd_url'), urlSource };
    const { container } = render(<SkillDraftCard draft={draft} onApprove={approve} onReject={vi.fn()} />);

    const source = within(screen.getByRole('note')).getByRole('link', { name: urlSource.sourceUrl });
    expect(source).toHaveAttribute('href', urlSource.sourceUrl);
    expect(source).toHaveAttribute('rel', 'noopener noreferrer');
    expect(screen.getByText(/referenced or selected files/)).toBeVisible();
    expect(screen.getByText(/does not represent a complete repository or an immutable commit/)).toBeVisible();
    expect(screen.getByText(/Imported files remain inactive until you keep this skill/)).toBeVisible();

    await userEvent.click(screen.getByText('Original package instructions and metadata'));
    const original = screen.getByLabelText('Original package instructions');
    expect(original).toHaveTextContent('custom: retained');
    expect(original.textContent).toContain('⟨U+FEFF⟩---⟨CR⟩⟨LF⟩\n');
    expect(original.textContent).toContain('<img src=x onerror=alert(1)>');
    expect(container.querySelector('img')).toBeNull();

    await userEvent.click(screen.getByText('Captured source details'));
    expect(screen.getByText(urlSource.packageHash)).toBeVisible();
    expect(screen.getByText(urlSource.files[0].sha256)).toBeVisible();
    expect(screen.getByText(urlSource.files[1].sha256)).toBeVisible();
    expect(screen.getByText('references/guide.md')).toBeVisible();
    expect(screen.getByText('45 bytes')).toBeVisible();
    expect(screen.getByRole('link', { name: urlSource.files[1].sourceUrl })).toBeVisible();
    expect(container.querySelector('time')).toHaveAttribute('datetime', '2026-11-01T03:04:05.000Z');
    expect(approve).not.toHaveBeenCalled();
    await userEvent.click(screen.getByRole('button', { name: 'Keep' }));
    expect(approve).toHaveBeenCalledWith(draft, {});
  });

  it('distinguishes the requested URL from a redirected captured document', () => {
    const urlSource = {
      ...urlSourceFixture(),
      requestedUrl: 'https://example.org/skills/research',
      sourceUrl: 'https://cdn.example.org/research/SKILL.md',
    };
    render(<SkillDraftCard draft={{ ...skillDraftFixture('skd_redirect'), urlSource }} onApprove={vi.fn()} onReject={vi.fn()} />);
    expect(screen.getByText(/Requested URL/)).toBeVisible();
    expect(screen.getByRole('link', { name: urlSource.requestedUrl })).toHaveAttribute('href', urlSource.requestedUrl);
    expect(screen.getByText(/redirected to a different source URL/)).toBeVisible();
    expect(screen.getByRole('link', { name: urlSource.sourceUrl })).toHaveAttribute('href', urlSource.sourceUrl);
  });

  it.each([
    'http://example.org/SKILL.md',
    'javascript:alert(1)',
    '//example.org/SKILL.md',
    'https:example.org/SKILL.md',
    'https://',
    'https://owner:secret@example.org/SKILL.md',
    'https://example.org/SKILL.md?token=private-secret',
    'https://example.org/SKILL.md#private-secret',
    'https://example.org/\nprivate-secret',
    'https://example.org\\@other.example/SKILL.md',
  ])('does not link or expose an unsafe source URL: %s', (unsafeUrl) => {
    const urlSource = {
      ...urlSourceFixture(),
      requestedUrl: unsafeUrl,
      sourceUrl: unsafeUrl,
      files: [{ path: 'SKILL.md', sourceUrl: unsafeUrl, sha256: 'e'.repeat(64), sizeBytes: 1 }],
    };
    const { container } = render(<SkillDraftCard draft={{ ...skillDraftFixture('skd_unsafe'), urlSource }} onApprove={vi.fn()} onReject={vi.fn()} />);
    expect(screen.queryByRole('link')).toBeNull();
    expect(screen.getAllByText('Source URL unavailable for safe linking.').length).toBeGreaterThan(0);
    expect(container.innerHTML).not.toContain(unsafeUrl);
    expect(container.innerHTML).not.toContain('private-secret');
    expect(container.innerHTML).not.toContain('owner:secret');
  });

  it('keeps rejected redirect origins hidden while exposing the safe final source', () => {
    const urlSource = {
      ...urlSourceFixture(),
      requestedUrl: 'https://example.org/SKILL.md?token=private-secret',
    };
    const { container } = render(<SkillDraftCard draft={{ ...skillDraftFixture('skd_unsafe_redirect'), urlSource }} onApprove={vi.fn()} onReject={vi.fn()} />);
    expect(screen.getByText(/redirected to a different source URL/)).toBeVisible();
    expect(within(screen.getByRole('note')).getByRole('link', { name: urlSource.sourceUrl })).toBeVisible();
    expect(container.innerHTML).not.toContain('private-secret');
  });

  it.each(['?token=private-secret', '#private-secret'])(
    'keeps URL parameters out of GitHub provenance links: %s',
    (suffix) => {
      const githubSource = {
        repository: 'example/notes',
        commit: '1'.repeat(40),
        path: 'skills/notes',
        sourceUrl: `https://github.com/example/notes/tree/${'1'.repeat(40)}/skills/notes${suffix}`,
        packageHash: 'a'.repeat(64),
        ignoredPaths: [],
      };
      const { container } = render(<SkillDraftCard draft={{ ...skillDraftFixture('skd_github_unsafe'), githubSource }} onApprove={vi.fn()} onReject={vi.fn()} />);
      expect(screen.queryByRole('link')).toBeNull();
      expect(screen.getByText(/example\/notes/)).toBeVisible();
      expect(container.innerHTML).not.toContain('private-secret');
    },
  );

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

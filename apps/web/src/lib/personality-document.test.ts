import { describe, expect, it } from 'vitest';
import {
  personalitySection,
  updatePersonalitySection,
} from './personality-document';

describe('personality document editing', () => {
  it('preserves frontmatter, prose, unknown sections, and neighboring fields', () => {
    const before =
      '---\nname: Nova\n---\n# Nova\nOriginal introduction.\n\n## Personality\nCurious.\n\n## Private tradition\nKeep this exactly.\n\n## Communication\nBe concise.\n';
    expect(personalitySection(before, 'personality')).toBe('Curious.');
    expect(
      updatePersonalitySection(
        before,
        'personality',
        'Curious and adventurous.',
      ),
    ).toBe(before.replace('Curious.', 'Curious and adventurous.'));
  });
  it('edits Alice-style plain headings without rewriting the rest', () => {
    const before =
      'Identity\n\nYou are Alice.\n\nPersonality\n\nCurious.\n\nPhilosophy\n\nFollow evidence.';
    expect(personalitySection(before, 'identity')).toBe('You are Alice.');
    expect(
      updatePersonalitySection(before, 'personality', 'Resourceful.'),
    ).toBe(before.replace('Curious.', 'Resourceful.'));
  });
  it('appends a missing field and keeps the document unchanged for an empty missing field', () => {
    expect(
      updatePersonalitySection(
        'Original prose.',
        'goals',
        'Find opportunities.',
      ),
    ).toBe('Original prose.\n\n## Goals and priorities\nFind opportunities.');
    expect(updatePersonalitySection('Original prose.\n', 'goals', '')).toBe(
      'Original prose.\n',
    );
  });
  it('keeps nested headings in the selected section and ignores code-fenced headings', () => {
    const before =
      '## Communication\nBe clear.\n### Formatting\nExplain simply.\n```md\n## Boundaries\nexample only\n```\n\n## Boundaries\nAsk before spending.';
    expect(personalitySection(before, 'communication')).toContain(
      'example only',
    );
    expect(personalitySection(before, 'boundaries')).toBe(
      'Ask before spending.',
    );
  });
  it('preserves spaces while typing a guided field and keeps empty sections separate', () => {
    let document = '## Personality\nCurious.\n\n## Boundaries\nAsk first.';
    document = updatePersonalitySection(
      document,
      'personality',
      'Curious and ',
    );
    expect(personalitySection(document, 'personality')).toBe('Curious and ');
    document = updatePersonalitySection(document, 'personality', '');
    document = updatePersonalitySection(document, 'personality', 'Warm.');
    expect(personalitySection(document, 'personality')).toBe('Warm.');
    expect(personalitySection(document, 'boundaries')).toBe('Ask first.');
  });
  it('inserts content safely into adjacent empty headings and a final heading without a newline', () => {
    const adjacent = updatePersonalitySection(
      '## Personality\n## Boundaries\nAsk first.',
      'personality',
      'Warm.',
    );
    expect(personalitySection(adjacent, 'personality')).toBe('Warm.');
    expect(personalitySection(adjacent, 'boundaries')).toBe('Ask first.');
    expect(
      updatePersonalitySection('## Personality', 'personality', 'Warm.'),
    ).toBe('## Personality\nWarm.');
  });
});

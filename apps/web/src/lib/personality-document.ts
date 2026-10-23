export const PERSONALITY_SECTIONS = [
  {
    id: 'identity',
    title: 'Identity and role',
    aliases: ['Identity', 'Who You Are', 'Who Am I?'],
    hint: 'Who are you, and what role do you play in my life or work?',
  },
  {
    id: 'personality',
    title: 'Personality and temperament',
    aliases: ['Personality', 'Your Personality', 'Temperament'],
    hint: 'Curious, thoughtful, adventurous, skeptical—describe how these traits show up.',
  },
  {
    id: 'values',
    title: 'Values and philosophy',
    aliases: ['Values', 'Philosophy', 'Core Truths'],
    hint: 'What matters to you? What principles guide your judgment?',
  },
  {
    id: 'goals',
    title: 'Goals and priorities',
    aliases: ['Goals', 'Your Role', 'Priorities'],
    hint: 'What should you work toward, and what takes priority?',
  },
  {
    id: 'expertise',
    title: 'Expertise',
    aliases: ['Your Expertise'],
    hint: 'Describe your areas of focus and the limits of your knowledge.',
  },
  {
    id: 'communication',
    title: 'Communication',
    aliases: ['How You Communicate', 'Vibe'],
    hint: 'Voice, brevity, humor, disagreement, and how you explain things.',
  },
  {
    id: 'boundaries',
    title: 'Boundaries',
    aliases: [],
    hint: 'When to pause, ask, or push back. Tool permissions are managed separately.',
  },
  {
    id: 'examples',
    title: 'Example replies',
    aliases: ['Examples', 'Example responses'],
    hint: 'Give a situation and the kind of reply you want to hear.',
  },
] as const;

export type PersonalitySectionId = (typeof PERSONALITY_SECTIONS)[number]['id'];
type Heading = { start: number; body: number; level: number; title: string };
const normalize = (value: string) => value.trim().toLowerCase();
const names = new Set(
  PERSONALITY_SECTIONS.flatMap((section) => [
    section.title,
    ...section.aliases,
  ]).map(normalize),
);

/** Locate sections without reserializing unrelated Markdown or frontmatter. */
function headings(document: string): Heading[] {
  const result: Heading[] = [];
  let offset = 0;
  let fence: { char: string; length: number } | undefined;
  let frontmatter = /^(?:\uFEFF)?---\r?\n/.test(document);
  let firstLine = true;
  for (const match of document.matchAll(/[^\n]*(?:\n|$)/g)) {
    const raw = match[0];
    if (!raw) continue;
    const line = raw.replace(/\r?\n$/, '');
    if (frontmatter) {
      if (!firstLine && /^(---|\.\.\.)\s*$/.test(line)) frontmatter = false;
    } else {
      const delimiter = line.match(/^ {0,3}(`{3,}|~{3,})/);
      if (delimiter) {
        const marker = delimiter[1];
        if (!fence) fence = { char: marker[0], length: marker.length };
        else if (
          marker[0] === fence.char &&
          marker.length >= fence.length &&
          line.trim() === marker
        )
          fence = undefined;
      } else if (!fence) {
        const markdown = line.match(/^ {0,3}(#{1,6})\s+(.+?)(?:\s+#+)?\s*$/);
        if (markdown)
          result.push({
            start: offset,
            body: offset + raw.length,
            level: markdown[1].length,
            title: markdown[2],
          });
        else if (
          names.has(normalize(line)) &&
          (offset === 0 || /\r?\n\r?\n$/.test(document.slice(0, offset)))
        ) {
          result.push({
            start: offset,
            body: offset + raw.length,
            level: 2,
            title: line,
          });
        }
      }
    }
    firstLine = false;
    offset += raw.length;
  }
  return result;
}

function locate(document: string, id: PersonalitySectionId) {
  const section = PERSONALITY_SECTIONS.find((section) => section.id === id)!;
  const aliases = [section.title, ...section.aliases].map(normalize);
  const all = headings(document);
  const index = all.findIndex((heading) =>
    aliases.includes(normalize(heading.title)),
  );
  if (index === -1) return undefined;
  const heading = all[index];
  const end =
    all
      .slice(index + 1)
      .find(
        (next) =>
          next.level <= heading.level || names.has(normalize(next.title)),
      )?.start ?? document.length;
  return {
    start: heading.body,
    end,
    needsNewline: !document.slice(heading.start, heading.body).endsWith('\n'),
  };
}

export function personalitySection(
  document: string,
  id: PersonalitySectionId,
): string {
  const range = locate(document, id);
  return range
    ? document
        .slice(range.start, range.end)
        .replace(/^(?:\r?\n)+|(?:\r?\n)+$/g, '')
    : '';
}

export function updatePersonalitySection(
  document: string,
  id: PersonalitySectionId,
  value: string,
): string {
  const range = locate(document, id);
  if (!range) {
    if (!value.trim()) return document;
    const title = PERSONALITY_SECTIONS.find(
      (section) => section.id === id,
    )!.title;
    const newline = document.includes('\r\n') ? '\r\n' : '\n';
    const separator = document.endsWith(newline + newline)
      ? ''
      : document.endsWith(newline)
        ? newline
        : document
          ? newline + newline
          : '';
    return `${document}${separator}## ${title}${newline}${value}`;
  }
  const current = document.slice(range.start, range.end);
  const empty = !current.replace(/\r?\n/g, '');
  const leading = empty ? '' : (current.match(/^(?:\r?\n)*/)?.[0] ?? '');
  let trailing = empty ? current : (current.match(/(?:\r?\n)*$/)?.[0] ?? '');
  const newline = document.includes('\r\n') ? '\r\n' : '\n';
  if (range.end < document.length && !trailing) trailing = newline + newline;
  return (
    document.slice(0, range.start) +
    (range.needsNewline ? newline : '') +
    leading +
    value +
    trailing +
    document.slice(range.end)
  );
}

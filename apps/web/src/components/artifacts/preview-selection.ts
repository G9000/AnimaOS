import { validPreviewEditContext, type OfficeTarget, type PreviewEditContext } from '@animaos/sdk';

/** Positions come from the server inventory; verify their raw bytes before narrowing them. */
export function htmlTextContext(path: string, sha256: string, scope: string, source: string, item: OfficeTarget, start: number, end: number): PreviewEditContext | null {
  if (!item.editable || item.value.kind !== 'html_text') return null;
  const value = item.value;
  if (![value.start_utf8, value.end_utf8, start, end].every(Number.isSafeInteger) || value.start_utf8 < 0 || value.end_utf8 <= value.start_utf8 || start < 0 || end <= start || end > value.text.length) return null;
  const bytes = new TextEncoder().encode(source);
  const original = new TextEncoder().encode(value.text);
  if (value.end_utf8 > bytes.length || value.end_utf8 - value.start_utf8 !== original.length || !original.every((byte, index) => byte === bytes[value.start_utf8 + index])) return null;
  const boundary = (index: number) => !(index > 0 && index < value.text.length && /[\ud800-\udbff]/.test(value.text[index - 1]) && /[\udc00-\udfff]/.test(value.text[index]));
  if (!boundary(start) || !boundary(end)) return null;
  const startUtf8 = value.start_utf8 + new TextEncoder().encode(value.text.slice(0, start)).length;
  const selected = value.text.slice(start, end);
  const context: PreviewEditContext = { version: 1, source: { kind: 'workspace', path, sha256, scope }, selection: { kind: 'html_text', start_utf8: startUtf8, end_utf8: startUtf8 + new TextEncoder().encode(selected).length, expected_text: selected } };
  return validPreviewEditContext(context) ? context : null;
}

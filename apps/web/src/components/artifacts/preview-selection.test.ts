import { describe, expect, it } from 'vitest';
import { htmlTextContext } from './preview-selection';
import type { OfficeTarget } from '@animaos/sdk';

describe('inert HTML selection provenance', () => {
  it('binds repeated text to its actual byte range and preserves Unicode boundaries', () => {
    const source = '<p>Hi</p><p>🙂 Hi</p>';
    const item: OfficeTarget = { id: 'html:12:20', label: 'Text 2', editable: true, value: { kind: 'html_text', start_utf8: 12, end_utf8: 19, text: '🙂 Hi' } };
    const context = htmlTextContext('report.html', 'a'.repeat(64), 'd'.repeat(64), source, item, 3, 5);
    expect(context?.selection).toEqual({ kind: 'html_text', start_utf8: 17, end_utf8: 19, expected_text: 'Hi' });
    expect(htmlTextContext('report.html', 'a'.repeat(64), 'd'.repeat(64), source, item, 1, 2)).toBeNull();
    expect(htmlTextContext('report.html', 'a'.repeat(64), 'd'.repeat(64), source, { ...item, value: { kind: 'html_text', start_utf8: 12, end_utf8: 19, text: 'changed' } }, 0, 2)).toBeNull();
    expect(htmlTextContext('report.html', 'a'.repeat(64), 'd'.repeat(64), source, { ...item, editable: false }, 0, 2)).toBeNull();
  });

  it('rejects large selections and invalid source bounds', () => {
    const source = `<p>${'a'.repeat(8193)}</p>`;
    const item: OfficeTarget = { id: 'html:3:8196', label: 'Text', editable: true, value: { kind: 'html_text', start_utf8: 3, end_utf8: 8196, text: 'a'.repeat(8193) } };
    expect(htmlTextContext('report.html', 'a'.repeat(64), 'd'.repeat(64), source, item, 0, 8193)).toBeNull();
    expect(htmlTextContext('report.html', 'a'.repeat(64), 'd'.repeat(64), source, { ...item, value: { kind: 'html_text', start_utf8: -1, end_utf8: 0, text: 'a' } }, 0, 1)).toBeNull();
  });
});

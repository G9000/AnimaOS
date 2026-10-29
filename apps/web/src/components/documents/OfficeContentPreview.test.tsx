import { fireEvent, render, screen, waitFor } from '@testing-library/react';
import { expect, it, vi } from 'vitest';
import type { OfficeInventoryPage, WorkspaceFileResponse } from '@animaos/sdk';
import { OfficeContentPreview } from './OfficeContentPreview';

const page: OfficeInventoryPage = { version: 1, source_scope: 'd'.repeat(64), format: 'docx', source_sha256: 'a'.repeat(64), source: { kind: 'workspace', display_name: 'report.docx' }, targets: [{ id: 'paragraph:1', label: 'Paragraph 1', editable: true, value: { kind: 'text', text: 'Original paragraph' } }, { id: 'preserved', label: 'Unsupported shape', editable: false, value: { kind: 'text', text: 'Keep unchanged' } }], coverage: { total_targets: 2, returned_targets: 2, complete: true }, retained_hyperlink_count: 0, limitations: ['Content preview only'], next_cursor: null };
const file: WorkspaceFileResponse = { path: 'report.docx', previewKind: 'office_content', sha256: page.source_sha256, content: '', truncated: false, officeContent: page };

it('offers only exact editable inventory targets and adds one after explicit action', async () => {
  const select = vi.fn();
  const view = render(<OfficeContentPreview path="report.docx" file={file} onSelect={select} />);
  fireEvent.click(screen.getByRole('button', { name: 'Select Paragraph 1' }));
  expect(select).not.toHaveBeenCalled();
  expect(screen.queryByRole('button', { name: 'Select Unsupported shape' })).toBeNull();
  fireEvent.click(screen.getByRole('button', { name: 'Add selection to draft' }));
  expect(select).toHaveBeenLastCalledWith({ version: 1, source: { kind: 'workspace', path: 'report.docx', sha256: page.source_sha256, scope: page.source_scope }, selection: { kind: 'office_target', format: 'docx', target_id: 'paragraph:1', label: 'Paragraph 1', value: page.targets[0].value } });
  view.unmount();
  expect(select).toHaveBeenLastCalledWith(null);
});

it('selects an exact HTML subrange from inert text and rejects stale source bytes', async () => {
  const html: OfficeInventoryPage = { ...page, format: 'html', source: { kind: 'workspace', display_name: 'report.html' }, targets: [{ id: 'html:3:8', label: 'Text 1', editable: true, value: { kind: 'html_text', start_utf8: 3, end_utf8: 8, text: 'Hello' } }], coverage: { total_targets: 1, returned_targets: 1, complete: true } };
  const htmlFile: WorkspaceFileResponse = { ...file, path: 'report.html', officeContent: html };
  const select = vi.fn();
  const { rerender } = render(<OfficeContentPreview path="report.html" file={htmlFile} htmlSource="<p>Hello</p>" onSelect={select} />);
  const text = screen.getByTestId('preview-target-text-html:3:8');
  const range = document.createRange(); range.setStart(text.firstChild!, 1); range.setEnd(text.firstChild!, 4);
  const selection = window.getSelection()!; selection.removeAllRanges(); selection.addRange(range);
  fireEvent.mouseUp(text);
  fireEvent.click(screen.getByRole('button', { name: 'Add selection to draft' }));
  expect(select).toHaveBeenLastCalledWith(expect.objectContaining({ selection: { kind: 'html_text', start_utf8: 4, end_utf8: 7, expected_text: 'ell' } }));
  rerender(<OfficeContentPreview path="report.html" file={htmlFile} htmlSource="<p>other</p>" onSelect={select} />);
  fireEvent.click(screen.getByRole('button', { name: 'Select Text 1' }));
  await waitFor(() => expect(screen.getByRole('button', { name: 'Add selection to draft' })).toBeDisabled());
});

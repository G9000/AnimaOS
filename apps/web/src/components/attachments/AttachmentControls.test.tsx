import { fireEvent, render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { expect, it, vi } from 'vitest';
import {
  AttachmentControls,
  attachmentDrop,
  attachmentPaste,
} from './AttachmentControls';
it('opens an accessible multiple picker, resets after selecting the same file and disables picking', async () => {
  const add = vi.fn();
  const file = new File(['x'], 'brief.txt');
  const { container, rerender } = render(
    <AttachmentControls disabled={false} onAdd={add} />,
  );
  const input = container.querySelector('input')!;
  expect(input.multiple).toBe(true);
  await userEvent.upload(input, file);
  expect(add).toHaveBeenLastCalledWith([file]);
  expect(input.value).toBe('');
  await userEvent.upload(input, file);
  expect(add).toHaveBeenCalledTimes(2);
  rerender(<AttachmentControls disabled onAdd={add} />);
  expect(screen.getByRole('button', { name: 'Attach files' })).toBeDisabled();
});
it('drop accepts only files and paste deduplicates file items without hijacking ordinary text', () => {
  const add = vi.fn();
  const file = new File(['x'], 'photo.png', { type: 'image/png' });
  const preventDefault = vi.fn();
  const data = {
    files: [file],
    items: [
      { kind: 'file', getAsFile: () => file },
      { kind: 'file', getAsFile: () => file },
    ],
  };
  attachmentDrop({ dataTransfer: data, preventDefault } as never, add, false);
  expect(add).toHaveBeenCalledWith([file]);
  add.mockClear();
  attachmentPaste({ clipboardData: data, preventDefault } as never, add, false);
  expect(add).toHaveBeenCalledOnce();
  expect(add).toHaveBeenCalledWith([file]);
  preventDefault.mockClear();
  attachmentDrop(
    { dataTransfer: { files: [] }, preventDefault } as never,
    add,
    false,
  );
  attachmentPaste(
    { clipboardData: { files: [], items: [] }, preventDefault } as never,
    add,
    false,
  );
  expect(preventDefault).not.toHaveBeenCalled();
  add.mockClear();
  attachmentDrop({ dataTransfer: data, preventDefault } as never, add, true);
  expect(add).not.toHaveBeenCalled();
});

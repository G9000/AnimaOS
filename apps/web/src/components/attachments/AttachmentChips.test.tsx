import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { expect, it, vi } from 'vitest';
import { AttachmentChips } from './AttachmentChips';
it('shows safe names, accessible measured progress and remove actions', async () => {
  const remove = vi.fn();
  render(
    <AttachmentChips
      items={[
        {
          localId: 'local',
          name: '<b>photo\u200B.png</b>',
          status: 'uploading',
          progress: 25,
          kind: 'image',
        },
      ]}
      onRemove={remove}
    />,
  );
  expect(screen.getByText('<b>photo⟨U+200B⟩.png</b>')).toBeVisible();
  const progress = screen.getByRole('progressbar');
  expect(progress).toHaveAttribute('aria-valuenow', '25');
  await userEvent.click(screen.getByRole('button', { name: /Remove/ }));
  expect(remove).toHaveBeenCalledWith('local');
});
it('uses indeterminate progress and safe error text', () => {
  render(
    <AttachmentChips
      items={[
        {
          localId: 'local',
          name: 'brief.txt',
          status: 'uploading',
          progress: null,
        },
        {
          localId: 'error',
          name: 'bad.txt',
          status: 'error',
          progress: null,
          error: '<script>bad</script>',
        },
      ]}
      onRemove={vi.fn()}
    />,
  );
  expect(screen.getByRole('progressbar')).not.toHaveAttribute('aria-valuenow');
  expect(screen.getByText('<script>bad</script>')).toBeVisible();
});

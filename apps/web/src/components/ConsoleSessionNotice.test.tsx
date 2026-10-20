import { act, fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';

import { LAUNCH_CODE_SPENT_WARNING } from '../lib/console-session';
import { ConsoleSessionNotice } from './ConsoleSessionNotice';

describe('ConsoleSessionNotice', () => {
  it('warns the owner when the launch link was already used', async () => {
    await act(async () => {
      render(
        <ConsoleSessionNotice
          claim={Promise.resolve({
            kind: 'refused',
            warning: LAUNCH_CODE_SPENT_WARNING,
          })}
        />,
      );
    });
    expect(screen.getByRole('alert').textContent).toContain(
      LAUNCH_CODE_SPENT_WARNING,
    );
    fireEvent.click(screen.getByLabelText('Dismiss'));
    expect(screen.queryByRole('alert')).toBeNull();
  });

  it('shows nothing when a session opened or there was no code', async () => {
    for (const kind of ['opened', 'none'] as const) {
      const { unmount } = await act(async () =>
        render(<ConsoleSessionNotice claim={Promise.resolve({ kind })} />),
      );
      expect(screen.queryByRole('alert')).toBeNull();
      unmount();
    }
  });
});

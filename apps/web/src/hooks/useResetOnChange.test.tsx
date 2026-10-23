import { useLayoutEffect, useState } from 'react';
import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import { useResetOnChange } from './useResetOnChange';

describe('useResetOnChange', () => {
  it('resets local state before committing children, preserving edits between input changes', () => {
    const committed: string[] = [];
    function Editor({ owner, online }: { owner: string; online: boolean }) {
      const [draft, setDraft] = useState('stale');
      useResetOnChange([owner, online], () => setDraft(`${owner}:saved`));
      useLayoutEffect(() => {
        committed.push(draft);
      }, [draft]);
      return (
        <input
          aria-label="Draft"
          value={draft}
          onChange={(event) => setDraft(event.target.value)}
        />
      );
    }
    const view = render(<Editor owner="alpha" online />);
    expect(committed).toEqual(['alpha:saved']);
    fireEvent.change(screen.getByLabelText('Draft'), {
      target: { value: 'my draft' },
    });
    view.rerender(<Editor owner="alpha" online />);
    expect(screen.getByLabelText('Draft')).toHaveValue('my draft');
    view.rerender(<Editor owner="beta" online />);
    expect(screen.getByLabelText('Draft')).toHaveValue('beta:saved');
    view.rerender(<Editor owner="alpha" online />);
    expect(screen.getByLabelText('Draft')).toHaveValue('alpha:saved');
    expect(committed).toEqual([
      'alpha:saved',
      'my draft',
      'beta:saved',
      'alpha:saved',
    ]);
  });
});

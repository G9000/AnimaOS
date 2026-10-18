import { render } from '@testing-library/react';
import { expect, it } from 'vitest';
import { HaloBackdrop } from './HaloBackdrop';

it('shows the owner’s words on the orb only while listening', () => {
  const view = render(<HaloBackdrop mode="listening" words="Move my 3pm" />);
  expect(view.container.querySelector('.halo-words')).toHaveTextContent(
    'Move my 3pm',
  );
  view.rerender(<HaloBackdrop mode="calm" words="Move my 3pm" />);
  expect(view.container.querySelector('.halo-words')).toBeNull();
});

it('falls back to a gradient orb without WebGL and stays hidden from assistive tech', () => {
  const view = render(<HaloBackdrop mode="attention" />);
  const backdrop = view.container.querySelector('.halo-backdrop');
  expect(backdrop).toHaveAttribute('aria-hidden');
  expect(backdrop).toHaveAttribute('data-mode', 'attention');
  expect(view.container.querySelector('.halo-orb')).toHaveAttribute(
    'data-fallback',
  );
});

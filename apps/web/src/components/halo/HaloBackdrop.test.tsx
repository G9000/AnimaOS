import { render } from '@testing-library/react';
import { expect, it } from 'vitest';
import { HaloBackdrop } from './HaloBackdrop';

it('falls back to a gradient orb without WebGL and stays hidden from assistive tech', () => {
  const view = render(<HaloBackdrop mode="attention" />);
  const backdrop = view.container.querySelector('.halo-backdrop');
  expect(backdrop).toHaveAttribute('aria-hidden');
  expect(backdrop).toHaveAttribute('data-mode', 'attention');
  expect(view.container.querySelector('.halo-orb')).toHaveAttribute(
    'data-fallback',
  );
});

it('rests the orb at the edge with an accent wash in the quiet background', () => {
  const view = render(<HaloBackdrop mode="calm" variant="quiet" />);
  expect(view.container.querySelector('.halo-backdrop')).toHaveAttribute(
    'data-variant',
    'quiet',
  );
  expect(view.container.querySelector('.halo-wash')).not.toBeNull();
  expect(view.container.querySelector('.halo-backdrop-orb')).not.toBeNull();
});

it('keeps the orb behind the chat in the full background, with no wash', () => {
  const view = render(<HaloBackdrop mode="calm" variant="full" />);
  expect(view.container.querySelector('.halo-wash')).toBeNull();
  expect(view.container.querySelector('.halo-backdrop-orb')).not.toBeNull();
});

it('draws nothing but the field when the background is off', () => {
  const view = render(<HaloBackdrop mode="thinking" variant="off" />);
  expect(view.container.querySelector('.halo-backdrop-orb')).toBeNull();
  expect(view.container.querySelector('.halo-glow')).toBeNull();
  expect(view.container.querySelector('.halo-wash')).toBeNull();
});

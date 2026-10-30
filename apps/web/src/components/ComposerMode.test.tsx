import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';

import { ComposerMode } from './ComposerMode';

const model = (overrides = {}) => ({
  provider: 'openai',
  current: 'gpt-4.1',
  onPick: vi.fn(),
  ...overrides,
});

describe('ComposerMode', () => {
  it('shows how it works and the model in one chip', () => {
    render(
      <ComposerMode
        mode={{ plan: false, canPlan: true, onChange: vi.fn() }}
        model={model()}
      />,
    );
    const chip = screen.getByRole('button', { name: /^Do, gpt-4\.1/ });
    expect(chip).toHaveTextContent('Do');
    expect(chip).toHaveTextContent('gpt-4.1');
  });

  it('plans first when Plan is chosen', async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(
      <ComposerMode
        mode={{ plan: false, canPlan: true, onChange }}
        model={model()}
      />,
    );
    await user.click(screen.getByRole('button', { name: /^Do, gpt-4\.1/ }));
    expect(screen.getByRole('menuitemradio', { name: /^Do/ })).toBeChecked();
    await user.click(screen.getByRole('menuitemradio', { name: /^Plan/ }));
    expect(onChange).toHaveBeenCalledWith(true);
    expect(screen.queryByRole('menu')).not.toBeInTheDocument();
  });

  it('shows Plan on the chip while planning', () => {
    render(
      <ComposerMode
        mode={{ plan: true, canPlan: true, onChange: vi.fn() }}
        model={model()}
      />,
    );
    expect(
      screen.getByRole('button', { name: /^Plan, gpt-4\.1/ }),
    ).toBeVisible();
  });

  it('keeps Plan off where the companion has no plan tool', async () => {
    const user = userEvent.setup();
    const onChange = vi.fn();
    render(
      <ComposerMode
        mode={{ plan: true, canPlan: false, onChange }}
        model={model()}
      />,
    );
    // A stale Plan choice reads as Do: it cannot plan.
    await user.click(screen.getByRole('button', { name: /^Do, gpt-4\.1/ }));
    const plan = screen.getByRole('menuitemradio', { name: /^Plan/ });
    expect(plan).toBeDisabled();
    expect(plan).toHaveTextContent('Needs the Edit files level');
    await user.click(plan);
    expect(onChange).not.toHaveBeenCalled();
  });

  it('switches model, but not to the one already in use', async () => {
    const user = userEvent.setup();
    const onPick = vi.fn();
    render(<ComposerMode model={model({ onPick })} />);
    await user.click(screen.getByRole('button', { name: /gpt-4\.1/ }));
    await user.click(screen.getByRole('menuitemradio', { name: 'gpt-4.1' }));
    expect(onPick).not.toHaveBeenCalled();
    await user.click(screen.getByRole('button', { name: /gpt-4\.1/ }));
    await user.click(screen.getByRole('menuitemradio', { name: 'gpt-4o' }));
    expect(onPick).toHaveBeenCalledWith('gpt-4o');
  });

  it('lists the current model even when no suggestion matches it', async () => {
    const user = userEvent.setup();
    render(<ComposerMode model={model({ current: 'gpt-5-custom' })} />);
    await user.click(screen.getByRole('button', { name: /gpt-5-custom/ }));
    expect(
      screen.getByRole('menuitemradio', { name: 'gpt-5-custom' }),
    ).toBeChecked();
  });

  it('closes on Escape and gives the focus back to the chip', async () => {
    const user = userEvent.setup();
    render(
      <ComposerMode
        mode={{ plan: false, canPlan: true, onChange: vi.fn() }}
        model={model()}
      />,
    );
    const chip = screen.getByRole('button', { name: /^Do, gpt-4\.1/ });
    await user.click(chip);
    expect(screen.getByRole('menu')).toBeVisible();
    await user.keyboard('{Escape}');
    expect(screen.queryByRole('menu')).not.toBeInTheDocument();
    expect(chip).toHaveFocus();
  });

  it('offers every other model in Settings and shows a failed save', async () => {
    const user = userEvent.setup();
    const onMore = vi.fn();
    render(
      <ComposerMode model={model({ onMore, error: 'daemon refused it' })} />,
    );
    await user.click(screen.getByRole('button', { name: /gpt-4\.1/ }));
    expect(screen.getByRole('alert')).toHaveTextContent('daemon refused it');
    await user.click(
      screen.getByRole('menuitem', { name: /All models and providers/ }),
    );
    expect(onMore).toHaveBeenCalledOnce();
  });

  it('renders nothing without a mode or a model', () => {
    const { container } = render(<ComposerMode />);
    expect(container).toBeEmptyDOMElement();
  });

  it('sets how hard the model thinks, with Auto for its default', async () => {
    const user = userEvent.setup();
    const onEffort = vi.fn();
    render(
      <ComposerMode
        model={model({
          efforts: ['low', 'medium', 'high'],
          effort: 'medium',
          onEffort,
        })}
      />,
    );
    await user.click(screen.getByRole('button', { name: /^Model, gpt-4\.1/ }));
    const thinking = screen.getByRole('radiogroup', { name: 'Thinking' });
    expect(screen.getByRole('radio', { name: 'Med' })).toBeChecked();
    expect(thinking).toHaveTextContent('Auto');
    await user.click(screen.getByRole('radio', { name: 'High' }));
    expect(onEffort).toHaveBeenLastCalledWith('high');
    await user.click(screen.getByRole('radio', { name: 'Auto' }));
    expect(onEffort).toHaveBeenLastCalledWith(null);
  });

  it('offers no thinking choice when the model has none', async () => {
    const user = userEvent.setup();
    render(<ComposerMode model={model({ efforts: [], onEffort: vi.fn() })} />);
    await user.click(screen.getByRole('button', { name: /^Model, gpt-4\.1/ }));
    expect(
      screen.queryByRole('radiogroup', { name: 'Thinking' }),
    ).not.toBeInTheDocument();
  });
});

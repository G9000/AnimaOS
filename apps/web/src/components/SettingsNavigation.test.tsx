import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { describe, expect, it, vi } from 'vitest';
import { DestinationNavigation } from './SettingsNavigation';

const defaults = {
  page: 'profile' as const,
  placement: 'sidebar' as const,
  navigate: vi.fn(),
  pendingApprovals: 0,
  locked: false,
};

describe('settings navigation', () => {
  it('keeps everyday settings visible and reveals technical pages on request', async () => {
    render(<DestinationNavigation {...defaults} />);
    expect(screen.getByRole('button', { name: 'Appearance' })).toBeVisible();
    expect(screen.getByRole('button', { name: 'Permissions' })).toBeVisible();
    expect(screen.queryByRole('button', { name: 'Runtime' })).toBeNull();
    const advanced = screen.getByRole('button', { name: 'Advanced' });
    await userEvent.click(advanced);
    expect(advanced).toHaveAttribute('aria-expanded', 'true');
    expect(screen.getByRole('button', { name: 'Runtime' })).toBeVisible();
    await userEvent.click(advanced);
    expect(screen.queryByRole('button', { name: 'Runtime' })).toBeNull();
  });

  it('reveals the selected advanced page after external navigation', () => {
    const view = render(<DestinationNavigation {...defaults} />);
    view.rerender(<DestinationNavigation {...defaults} page="logs" />);
    expect(screen.getByRole('button', { name: 'Logs' })).toHaveAttribute(
      'aria-current',
      'page',
    );
    expect(screen.getByRole('button', { name: 'Advanced' })).toHaveAttribute(
      'aria-expanded',
      'true',
    );
  });

  it('searches hidden settings without permanently expanding the group', () => {
    const view = render(
      <DestinationNavigation {...defaults} query="diagnostics" />,
    );
    expect(screen.getByRole('button', { name: 'Diagnostics' })).toBeVisible();
    expect(screen.queryByRole('button', { name: 'Profile' })).toBeNull();
    view.rerender(<DestinationNavigation {...defaults} />);
    expect(screen.getByRole('button', { name: 'Advanced' })).toHaveAttribute(
      'aria-expanded',
      'false',
    );
    expect(screen.queryByRole('button', { name: 'Diagnostics' })).toBeNull();
  });

  it('keeps pending reviews discoverable when advanced settings are closed', async () => {
    render(
      <DestinationNavigation
        {...defaults}
        pendingApprovals={2}
        pendingSkills={3}
      />,
    );
    expect(
      screen.getByRole('button', { name: 'Permissions, 2 waiting' }),
    ).toBeVisible();
    await userEvent.click(
      screen.getByRole('button', { name: 'Advanced, 3 waiting' }),
    );
    expect(
      screen.getByRole('button', { name: 'Skills, 3 waiting' }),
    ).toBeVisible();
  });

  it('offers every destination in a mobile picker and preserves waiting counts', async () => {
    const navigate = vi.fn();
    render(
      <DestinationNavigation
        {...defaults}
        navigate={navigate}
        placement="mobile"
        pendingApprovals={2}
      />,
    );
    const picker = screen.getByRole('combobox', { name: 'Settings section' });
    expect(picker).toHaveValue('profile');
    expect(
      screen.getByRole('option', { name: 'Permissions · 2 waiting' }),
    ).toBeInTheDocument();
    await userEvent.selectOptions(picker, 'logs');
    expect(navigate).toHaveBeenCalledWith({ kind: 'page', page: 'logs' });
  });

  it('locks the mobile picker while a settings change is saving', () => {
    render(<DestinationNavigation {...defaults} placement="mobile" locked />);
    expect(
      screen.getByRole('combobox', { name: 'Settings section' }),
    ).toBeDisabled();
  });
});

import { render, screen } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { NotificationsPage } from './NotificationsPage';

function stubNotification(permission: NotificationPermission) {
  const created: string[] = [];
  class FakeNotification {
    static permission = permission;
    static requestPermission = vi.fn(async () => {
      FakeNotification.permission = 'granted';
      return 'granted' as const;
    });
    onclick: (() => void) | null = null;
    constructor(title: string) {
      created.push(title);
    }
    close() {}
  }
  vi.stubGlobal('Notification', FakeNotification);
  return { created, FakeNotification };
}

afterEach(() => vi.unstubAllGlobals());

describe('NotificationsPage', () => {
  it('asks only when the owner turns notifications on, then offers a test', async () => {
    const user = userEvent.setup();
    const { created, FakeNotification } = stubNotification('default');
    render(<NotificationsPage />);
    expect(FakeNotification.requestPermission).not.toHaveBeenCalled();
    expect(screen.getByRole('status')).toHaveTextContent('Off.');

    await user.click(screen.getByRole('button', { name: 'Turn on' }));
    expect(FakeNotification.requestPermission).toHaveBeenCalledOnce();
    expect(screen.getByRole('status')).toHaveTextContent('On for this browser');

    await user.click(screen.getByRole('button', { name: 'Send a test' }));
    expect(created).toEqual(['Notifications are on']);
  });

  it('explains a browser block and says what is not available yet', () => {
    stubNotification('denied');
    render(<NotificationsPage />);
    expect(screen.getByRole('status')).toHaveTextContent('Blocked by your browser');
    expect(
      screen.queryByRole('button', { name: 'Turn on' }),
    ).not.toBeInTheDocument();
    expect(screen.getByText(/Not available yet/)).toBeVisible();
  });
});

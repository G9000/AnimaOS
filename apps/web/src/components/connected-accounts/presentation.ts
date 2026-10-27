export const accountButton =
  'rounded-lg border border-line px-3 py-2 text-xs hover:bg-white/[0.04] disabled:opacity-50';
export const providerName = (value: string) =>
  ({
    gmail: 'Gmail',
    outlook: 'Outlook',
    email: 'Email',
    gdrive: 'Google Drive',
    gcalendar: 'Google Calendar',
  })[value] ?? value;
export const sourceStatus = (value: string) =>
  ({
    disabled: 'Not enabled',
    ready: 'Ready',
    syncing: 'Waiting for sync',
    stale: 'Refresh needed',
    reauthRequired: 'Reconnect required',
    unavailable: 'Account or read permission unavailable',
  })[value] ?? value;
export const workflowStatus = (value: string) =>
  ({
    running: 'Running',
    pending: 'Pending',
    awaiting_approval: 'Awaiting approval',
    awaiting_input: 'Awaiting your answer',
    needs_review: 'Needs review',
    completed: 'Completed',
    cancelled: 'Cancelled',
  })[value] ?? value;

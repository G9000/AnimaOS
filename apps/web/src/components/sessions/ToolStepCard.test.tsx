import { fireEvent, render, screen } from '@testing-library/react';
import { describe, expect, it } from 'vitest';
import type { ToolStep } from '../../lib/transcript';
import { ToolStepCard } from './ToolStepCard';

describe('m83 Search sources card', () => {
  const sources = [
    {
      title: '<img src=x onerror=alert(1)> returned title',
      url: 'https://example.com/returned?complete=yes&part=2',
    },
  ];
  const step = {
    stepId: 'run:1',
    toolCallId: 'call',
    runId: 'run',
    name: 'web_search',
    argumentsPreview: 'bounded query',
    status: 'success' as const,
    durationMs: 12,
    result: 'x'.repeat(2048),
    truncated: true,
    helper: null,
    sources,
  };

  it('renders exact source links as text outside the clipped result', () => {
    const { container } = render(<ToolStepCard step={step} />);
    fireEvent.click(screen.getByRole('button', { name: /web_search/ }));
    expect(screen.getByText('Search sources')).toBeVisible();
    const link = screen.getByRole('link', { name: sources[0].title });
    expect(link).toHaveAttribute('href', sources[0].url);
    expect(link).toHaveAttribute('target', '_blank');
    expect(link).toHaveAttribute('rel', expect.stringContaining('noreferrer'));
    expect(link.closest('pre')).toBeNull();
    expect(container.querySelector('img')).toBeNull();
    expect(screen.getByText('Result shortened to 2 KiB.')).toBeVisible();
  });

  it.each(['running', 'error'] as const)(
    'never displays sources for a %s card',
    (status) => {
      render(<ToolStepCard step={{ ...step, status }} />);
      fireEvent.click(screen.getByRole('button', { name: /web_search/ }));
      expect(screen.queryByText('Search sources')).not.toBeInTheDocument();
      expect(screen.queryByRole('link')).not.toBeInTheDocument();
    },
  );

  it('clears visible sources when the same mounted card changes status or loses its projection', () => {
    const view = render(<ToolStepCard step={step} />);
    fireEvent.click(screen.getByRole('button', { name: /web_search/ }));
    expect(screen.getByRole('link')).toHaveAttribute('href', sources[0].url);
    view.rerender(<ToolStepCard step={{ ...step, status: 'error' }} />);
    expect(screen.queryByRole('link')).not.toBeInTheDocument();
    const legacy: ToolStep = { ...step };
    delete (legacy as ToolStep & { sources?: unknown }).sources;
    view.rerender(<ToolStepCard step={legacy} />);
    expect(screen.queryByRole('link')).not.toBeInTheDocument();
  });

  it('rejects a direct unsafe source instead of making it clickable', () => {
    render(
      <ToolStepCard
        step={{
          ...step,
          sources: [{ title: 'Unsafe', url: 'javascript:alert(1)' }],
        }}
      />,
    );
    fireEvent.click(screen.getByRole('button', { name: /web_search/ }));
    expect(screen.queryByRole('link')).not.toBeInTheDocument();
  });
});

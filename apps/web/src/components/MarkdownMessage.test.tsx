import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from '@testing-library/react';
import { useState } from 'react';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { MarkdownMessage } from './MarkdownMessage';
import { WorkspaceArtifactLayout } from './artifacts/WorkspaceArtifactPreview';
import { ChatExportScope } from './sessions/ChatExportDownload';
import { daemon } from '../lib/daemon-api';

/** Counts Markdown parses; the real renderer still renders. */
const parses = vi.hoisted(() => vi.fn());
const documentOwnerScope = { agentId: 'a', sessionId: 'current' };
vi.mock('react-markdown', async (importOriginal) => {
  const actual = await importOriginal<typeof import('react-markdown')>();
  const Parsed = (props: Parameters<typeof actual.default>[0]) => {
    parses();
    return <actual.default {...props} />;
  };
  return { ...actual, default: Parsed };
});

describe('MarkdownMessage', () => {
  it('turns an inline HTML artifact path into an openable file reference', () => {
    render(
      <WorkspaceArtifactLayout
        agentId="a"
        sessionId="current"
        clearGeneration={0}
      >
        <MarkdownMessage>
          {'Created at `artifacts/credit-card-bill-check.html`.'}
        </MarkdownMessage>
      </WorkspaceArtifactLayout>,
    );
    expect(
      screen.getByRole('button', {
        name: 'Open artifacts/credit-card-bill-check.html',
      }),
    ).toBeVisible();
  });

  it('downloads a finished file through the owner transport only after a click', async () => {
    const download = vi
      .spyOn(daemon, 'downloadWorkspaceFile')
      .mockResolvedValue(
        new Blob(['document'], { type: 'application/octet-stream' }),
      );
    const click = vi
      .spyOn(HTMLAnchorElement.prototype, 'click')
      .mockImplementation(() => {});
    const revoke = vi
      .spyOn(URL, 'revokeObjectURL')
      .mockImplementation(() => {});
    vi.spyOn(URL, 'createObjectURL').mockReturnValue('blob:finished-chat-file');
    render(
      <ChatExportScope.Provider value={documentOwnerScope}>
        <MarkdownMessage>
          {
            '[Download report](/api/workspace/file/download?path=reports%2Fannual%20plan.pdf)'
          }
        </MarkdownMessage>
      </ChatExportScope.Provider>,
    );
    expect(download).not.toHaveBeenCalled();
    fireEvent.click(screen.getByRole('button', { name: 'Download report' }));
    await act(async () => {});
    expect(download).toHaveBeenCalledWith(
      'reports/annual plan.pdf',
      expect.any(AbortSignal),
    );
    expect(click.mock.instances[0]).toHaveProperty(
      'download',
      'annual plan.pdf',
    );
    await waitFor(() =>
      expect(revoke).toHaveBeenCalledWith('blob:finished-chat-file'),
    );
  });

  it('leaves a finished-file link inert outside an owner conversation', () => {
    const download = vi.spyOn(daemon, 'downloadWorkspaceFile');
    render(
      <MarkdownMessage>
        {
          '[Download report](/api/workspace/file/download?path=reports%2Fannual.pdf)'
        }
      </MarkdownMessage>,
    );
    expect(
      screen.queryByRole('button', { name: 'Download report' }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole('link', { name: 'Download report' }),
    ).not.toBeInTheDocument();
    fireEvent.click(screen.getByText('Download report'));
    expect(download).not.toHaveBeenCalled();
  });

  it('aborts a finished-file download and ignores its bytes after the conversation closes', async () => {
    let finish!: (blob: Blob) => void;
    const download = vi
      .spyOn(daemon, 'downloadWorkspaceFile')
      .mockImplementation(
        () =>
          new Promise((resolve) => {
            finish = resolve;
          }),
      );
    const create = vi.spyOn(URL, 'createObjectURL');
    const view = render(
      <ChatExportScope.Provider value={documentOwnerScope}>
        <MarkdownMessage>
          {
            '[Download report](/api/workspace/file/download?path=reports%2Fannual.pdf)'
          }
        </MarkdownMessage>
      </ChatExportScope.Provider>,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Download report' }));
    const signal = download.mock.calls[0][1]!;
    view.unmount();
    expect(signal.aborted).toBe(true);
    await act(async () => {
      finish(new Blob(['late file']));
    });
    expect(create).not.toHaveBeenCalled();
  });
  it('keeps a forged export link to another conversation inert', () => {
    const read = vi.spyOn(daemon, 'exportSession');
    render(
      <ChatExportScope.Provider value={{ agentId: 'a', sessionId: 'current' }}>
        <MarkdownMessage>
          {'[Download this chat](/api/agents/a/sessions/other/export)'}
        </MarkdownMessage>
      </ChatExportScope.Provider>,
    );
    expect(
      screen.queryByRole('button', { name: 'Download this chat' }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole('link', { name: 'Download this chat' }),
    ).not.toBeInTheDocument();
    fireEvent.click(screen.getByText('Download this chat'));
    expect(read).not.toHaveBeenCalled();
  });
  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
  });

  it('parses the same text once however often its parent renders', () => {
    function Parent() {
      const [renders, setRenders] = useState(0);
      const [text, setText] = useState('**steady**');
      return (
        <>
          <button type="button" onClick={() => setRenders(renders + 1)}>
            Render {renders}
          </button>
          <button type="button" onClick={() => setText('**changed**')}>
            Change
          </button>
          <MarkdownMessage>{text}</MarkdownMessage>
        </>
      );
    }
    parses.mockClear();
    render(<Parent />);
    expect(parses).toHaveBeenCalledTimes(1);

    for (let index = 0; index < 3; index += 1)
      fireEvent.click(screen.getByRole('button', { name: /^Render/ }));
    expect(parses).toHaveBeenCalledTimes(1);

    fireEvent.click(screen.getByRole('button', { name: 'Change' }));
    expect(parses).toHaveBeenCalledTimes(2);
    expect(screen.getByText('changed').tagName).toBe('STRONG');
  });

  it('renders safe GitHub-flavored Markdown semantics', () => {
    render(
      <MarkdownMessage>
        {
          '# Heading\n\n**bold** and ~~removed~~\n\n- one\n- [x] done\n\n1. first\n2. second\n\n> quoted\n\n| Name | Value |\n| --- | --- |\n| alpha | beta |'
        }
      </MarkdownMessage>,
    );

    expect(screen.getByTestId('markdown-message')).toBeVisible();
    expect(screen.getByRole('heading', { name: 'Heading' })).toBeVisible();
    expect(screen.getByText('bold').tagName).toBe('STRONG');
    expect(screen.getByText('removed').tagName).toBe('DEL');
    expect(screen.getAllByRole('list')).toHaveLength(2);
    expect(screen.getByRole('checkbox')).toBeChecked();
    expect(screen.getByRole('blockquote')).toHaveTextContent('quoted');
    expect(screen.getByRole('table')).toHaveTextContent('alpha');
  });

  it('keeps raw HTML inert and unsafe link protocols non-executable', () => {
    render(
      <MarkdownMessage>
        {
          '<script data-testid="raw-script">alert(1)</script>\n\n[unsafe](javascript:alert(1))'
        }
      </MarkdownMessage>,
    );

    expect(screen.getByText(/alert\(1\)/)).toBeVisible();
    expect(screen.queryByTestId('raw-script')).not.toBeInTheDocument();
    const unsafe = screen.getByText('unsafe');
    expect(unsafe).not.toHaveAttribute('href', 'javascript:alert(1)');
  });

  it('opens absolute web links safely and retains relative links in the current tab', () => {
    render(
      <MarkdownMessage>
        {'[docs](https://example.com/docs) [settings](/settings)'}
      </MarkdownMessage>,
    );

    expect(screen.getByRole('link', { name: 'docs' })).toHaveAttribute(
      'target',
      '_blank',
    );
    expect(screen.getByRole('link', { name: 'docs' })).toHaveAttribute(
      'rel',
      'noopener noreferrer',
    );
    expect(screen.getByRole('link', { name: 'settings' })).toHaveAttribute(
      'href',
      '/settings',
    );
    expect(screen.getByRole('link', { name: 'settings' })).not.toHaveAttribute(
      'target',
    );
  });

  it('contains wide tables and wraps long inline code', () => {
    render(
      <MarkdownMessage>
        {
          '`a-very-long-inline-code-value`\n\n| A | B |\n| --- | --- |\n| 1 | 2 |'
        }
      </MarkdownMessage>,
    );

    const tableWrapper = screen.getByRole('table').parentElement;
    expect(tableWrapper).toHaveAttribute('data-markdown-overflow', 'table');
    expect(tableWrapper).toHaveClass('max-w-full', 'overflow-x-auto');
    // Long values still wrap, but at a break opportunity first: break-all
    // split ordinary words ("Sto/p") mid-word.
    expect(screen.getByText('a-very-long-inline-code-value')).toHaveClass(
      '[overflow-wrap:anywhere]',
    );
  });

  it('renders Markdown images as inert accessible placeholders', () => {
    render(
      <MarkdownMessage>
        {'![tracking pixel](https://example.com/pixel.gif)'}
      </MarkdownMessage>,
    );

    expect(screen.queryByRole('img')).not.toBeInTheDocument();
    expect(screen.getByText('[Image: tracking pixel]')).toBeVisible();
  });

  it('uses a neutral placeholder when a Markdown image has no alt text', () => {
    render(
      <MarkdownMessage>{'![](https://example.com/pixel.gif)'}</MarkdownMessage>,
    );

    expect(screen.queryByRole('img')).not.toBeInTheDocument();
    expect(screen.getByText('[Image]')).toBeVisible();
  });

  it('preserves ordered-list starts, link titles, and table alignment', () => {
    render(
      <MarkdownMessage>
        {
          '3. third\n4. fourth\n\n[reference](https://example.com "Reference title")\n\n| Left | Center | Right |\n| :--- | :----: | ----: |\n| a | b | c |'
        }
      </MarkdownMessage>,
    );

    expect(screen.getByRole('list')).toHaveAttribute('start', '3');
    expect(screen.getByRole('link', { name: 'reference' })).toHaveAttribute(
      'title',
      'Reference title',
    );
    const centerHeader = screen.getByRole('columnheader', { name: 'Center' });
    const rightCell = screen.getByRole('cell', { name: 'c' });
    expect(centerHeader).toHaveStyle({ textAlign: 'center' });
    expect(rightCell).toHaveStyle({
      textAlign: 'right',
    });
    expect(centerHeader).not.toHaveAttribute('align');
    expect(rightCell).not.toHaveAttribute('align');
  });

  it('keeps inline code free of a copy control', () => {
    render(<MarkdownMessage>{'Use `inlineValue` here.'}</MarkdownMessage>);

    expect(screen.getByText('inlineValue')).toBeVisible();
    expect(
      screen.queryByRole('button', { name: /copy/i }),
    ).not.toBeInTheDocument();
  });

  it('preserves GFM task-list classes without generic list bullets', () => {
    render(
      <MarkdownMessage>{'- [ ] pending\n- [x] complete'}</MarkdownMessage>,
    );

    expect(screen.getByRole('list')).toHaveClass('contains-task-list');
    expect(screen.getByRole('list')).not.toHaveClass('list-disc');
    expect(screen.getAllByRole('listitem')[0]).toHaveClass('task-list-item');
  });

  it('labels fenced TypeScript and copies its exact source', async () => {
    const writeText = vi.fn(async () => undefined);
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText },
    });
    render(
      <MarkdownMessage>
        {'```typescript\nconst answer = 42;\n```'}
      </MarkdownMessage>,
    );

    expect(screen.getByText('typescript')).toBeVisible();
    const copy = screen.getByRole('button', { name: 'Copy' });
    fireEvent.click(copy);

    expect(writeText).toHaveBeenCalledWith('const answer = 42;\n');
    expect(await screen.findByRole('button', { name: 'Copied' })).toBeVisible();
    expect(screen.getByRole('status')).toHaveTextContent('Copied');
    expect(
      screen.getByTestId('markdown-message').querySelectorAll('pre'),
    ).toHaveLength(1);
    const codeWrapper = screen
      .getByTestId('markdown-message')
      .querySelector('[data-markdown-overflow="code"]');
    expect(codeWrapper).toBeVisible();
    expect(codeWrapper).toHaveClass('max-w-full', 'overflow-x-auto');
  });

  it('contains a long fence-language label without shrinking Copy', () => {
    const language = 'very-long-custom-language-token-for-narrow-chat-bubbles';
    render(
      <MarkdownMessage>{`\`\`\`${language}\nconst value = 1;\n\`\`\``}</MarkdownMessage>,
    );

    expect(screen.getByText(language)).toHaveClass('min-w-0', 'truncate');
    expect(screen.getByRole('button', { name: 'Copy' })).toHaveClass(
      'shrink-0',
    );
  });

  it('applies Prism token classes for supported fenced TypeScript', () => {
    render(
      <MarkdownMessage>
        {'```typescript\nconst answer = 42;\n```'}
      </MarkdownMessage>,
    );

    expect(screen.getByText('const')).toHaveClass('token', 'keyword');
  });

  it('restores Copy after two seconds', async () => {
    vi.useFakeTimers();
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: { writeText: vi.fn(async () => undefined) },
    });
    render(
      <MarkdownMessage>{'```ts\nconst answer = 42;\n```'}</MarkdownMessage>,
    );

    fireEvent.click(screen.getByRole('button', { name: 'Copy' }));
    await act(async () => {
      await Promise.resolve();
    });
    expect(screen.getByRole('button', { name: 'Copied' })).toBeVisible();
    act(() => vi.advanceTimersByTime(2_000));
    expect(screen.getByRole('button', { name: 'Copy' })).toBeVisible();
  });

  it('shows a failure label when Clipboard is unavailable or rejects', async () => {
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: undefined,
    });
    const { unmount } = render(
      <MarkdownMessage>{'```text\nhello\n```'}</MarkdownMessage>,
    );
    fireEvent.click(screen.getByRole('button', { name: 'Copy' }));
    expect(
      await screen.findByRole('button', { name: 'Copy failed' }),
    ).toBeVisible();
    unmount();

    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: {
        writeText: vi.fn(async () => Promise.reject(new Error('denied'))),
      },
    });
    render(<MarkdownMessage>{'```text\nhello\n```'}</MarkdownMessage>);
    fireEvent.click(screen.getByRole('button', { name: 'Copy' }));
    expect(
      await screen.findByRole('button', { name: 'Copy failed' }),
    ).toBeVisible();
  });

  it('does not schedule clipboard feedback after unmounting during a pending copy', async () => {
    vi.useFakeTimers();
    let resolveWrite: (() => void) | undefined;
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: {
        writeText: vi.fn(
          () =>
            new Promise<void>((resolve) => {
              resolveWrite = resolve;
            }),
        ),
      },
    });
    const { unmount } = render(
      <MarkdownMessage>{'```text\nhello\n```'}</MarkdownMessage>,
    );

    fireEvent.click(screen.getByRole('button', { name: 'Copy' }));
    unmount();
    resolveWrite?.();
    await act(async () => {
      await Promise.resolve();
    });

    expect(vi.getTimerCount()).toBe(0);
  });

  it('invalidates a pending copy when fenced source changes', async () => {
    vi.useFakeTimers();
    let resolveWrite: (() => void) | undefined;
    Object.defineProperty(navigator, 'clipboard', {
      configurable: true,
      value: {
        writeText: vi.fn(
          () =>
            new Promise<void>((resolve) => {
              resolveWrite = resolve;
            }),
        ),
      },
    });
    const { rerender } = render(
      <MarkdownMessage>{'```text\nold source\n```'}</MarkdownMessage>,
    );

    fireEvent.click(screen.getByRole('button', { name: 'Copy' }));
    rerender(
      <MarkdownMessage>{'```typescript\nnew source\n```'}</MarkdownMessage>,
    );
    resolveWrite?.();
    await act(async () => {
      await Promise.resolve();
    });

    expect(screen.getByText('typescript')).toBeVisible();
    expect(screen.getByRole('button', { name: 'Copy' })).toBeVisible();
    expect(vi.getTimerCount()).toBe(0);
  });

  it('falls back to original source for an unknown fenced language', () => {
    render(
      <MarkdownMessage>
        {'```not-a-real-language\nkeep <this> source\n```'}
      </MarkdownMessage>,
    );

    expect(screen.getByText('not-a-real-language')).toBeVisible();
    expect(screen.getByText('keep <this> source')).toBeVisible();
  });
});

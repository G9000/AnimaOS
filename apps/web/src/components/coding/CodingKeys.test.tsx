import { render, screen, waitFor, within } from '@testing-library/react';
import userEvent from '@testing-library/user-event';
import { afterEach, describe, expect, it, vi } from 'vitest';

import { daemon, type CodingKey } from '../../lib/daemon-api';
import { CodingKeys } from './CodingKeys';

const keys: CodingKey[] = [
  { name: 'ANTHROPIC_API_KEY', label: 'Anthropic API key', saved: true, enabled: true },
  { name: 'OPENAI_API_KEY', label: 'OpenAI API key', saved: true, enabled: false },
  { name: 'GEMINI_API_KEY', label: 'Gemini API key', saved: false, enabled: false },
];

const toggle = (label: string) =>
  screen.getByRole('switch', { name: `Hand ${label} to coding apps` });

afterEach(() => vi.restoreAllMocks());

describe('CodingKeys', () => {
  it('lists the keys, marks unsaved ones and keeps them off', async () => {
    vi.spyOn(daemon, 'codingKeys').mockResolvedValue({ keys });
    render(<CodingKeys agentId="agent-1" />);
    const list = await screen.findByRole('list', { name: 'Keys for coding apps' });
    const rows = within(list).getAllByRole('listitem');
    expect(rows.map((row) => row.textContent)).toEqual([
      expect.stringContaining('ANTHROPIC_API_KEYSaved'),
      expect.stringContaining('OPENAI_API_KEYSaved'),
      expect.stringContaining('GEMINI_API_KEYNot saved'),
    ]);
    expect(toggle('Anthropic API key')).toHaveAttribute('aria-checked', 'true');
    expect(toggle('OpenAI API key')).toBeEnabled();
    expect(toggle('Gemini API key')).toBeDisabled();
    expect(
      screen.getByText(/Keys come from Settings › Model\. A key that isn't saved can't be turned on\./),
    ).toBeVisible();
  });

  it('sends the whole enabled set when a key is turned on and shows what the daemon kept', async () => {
    vi.spyOn(daemon, 'codingKeys').mockResolvedValue({ keys });
    const save = vi.spyOn(daemon, 'setCodingKeys').mockResolvedValue({
      keys: keys.map((key) =>
        key.name === 'OPENAI_API_KEY' ? { ...key, enabled: true } : key,
      ),
    });
    render(<CodingKeys agentId="agent-1" />);
    const user = userEvent.setup();
    await user.click(await screen.findByRole('switch', { name: /OpenAI API key/ }));
    expect(save).toHaveBeenCalledWith('agent-1', ['ANTHROPIC_API_KEY', 'OPENAI_API_KEY']);
    await waitFor(() => expect(toggle('OpenAI API key')).toBeEnabled());
    expect(toggle('OpenAI API key')).toHaveAttribute('aria-checked', 'true');
  });

  it('puts a key back and says why when the daemon refuses', async () => {
    vi.spyOn(daemon, 'codingKeys').mockResolvedValue({ keys });
    vi.spyOn(daemon, 'setCodingKeys').mockRejectedValue(new Error('ANTHROPIC_API_KEY is not saved'));
    render(<CodingKeys agentId="agent-1" />);
    const user = userEvent.setup();
    await user.click(await screen.findByRole('switch', { name: /Anthropic API key/ }));
    expect(await screen.findByRole('alert')).toHaveTextContent('ANTHROPIC_API_KEY is not saved');
    expect(toggle('Anthropic API key')).toHaveAttribute('aria-checked', 'true');
  });
});

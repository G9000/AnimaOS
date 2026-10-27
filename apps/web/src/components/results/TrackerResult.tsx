import type { TrackerPayload } from '@animaos/sdk';

const inputClass =
  'w-full min-w-28 rounded border border-line bg-panel px-2 py-1 text-sm disabled:opacity-60';

/** Native controls render saved data; result content never runs as code. */
export function TrackerResult({
  payload,
  onChange,
  disabled = false,
}: {
  payload: TrackerPayload;
  onChange?: (payload: TrackerPayload) => void;
  disabled?: boolean;
}) {
  function cell(rowId: string, key: string, value: string | number | boolean) {
    onChange?.({
      ...payload,
      rows: payload.rows.map((row) =>
        row.id === rowId
          ? { ...row, cells: { ...row.cells, [key]: value } }
          : row,
      ),
    });
  }
  return (
    <div className="space-y-3">
      <div className="overflow-x-auto">
        <table className="w-full text-left text-sm">
          <thead>
            <tr>
              {payload.columns.map((column) => (
                <th
                  key={column.key}
                  scope="col"
                  className="border-b border-line p-2 font-medium"
                >
                  {column.label}
                </th>
              ))}
              {onChange && (
                <th scope="col">
                  <span className="sr-only">Rows</span>
                </th>
              )}
            </tr>
          </thead>
          <tbody>
            {payload.rows.map((row, index) => (
              <tr key={row.id}>
                {payload.columns.map((column) => (
                  <td
                    key={column.key}
                    className="border-b border-line p-2 align-top"
                  >
                    {onChange ? (
                      column.kind === 'checkbox' ? (
                        <input
                          type="checkbox"
                          aria-label={`${column.label}, row ${index + 1}`}
                          checked={row.cells[column.key] === true}
                          disabled={disabled}
                          onChange={(event) =>
                            cell(row.id, column.key, event.target.checked)
                          }
                        />
                      ) : (
                        <input
                          type={column.kind === 'number' ? 'number' : 'text'}
                          step={column.kind === 'number' ? 'any' : undefined}
                          aria-label={`${column.label}, row ${index + 1}`}
                          maxLength={column.kind === 'text' ? 2048 : undefined}
                          value={String(row.cells[column.key] ?? '')}
                          disabled={disabled}
                          className={inputClass}
                          onChange={(event) =>
                            cell(
                              row.id,
                              column.key,
                              column.kind === 'number' &&
                                event.target.value.trim() !== ''
                                ? Number(event.target.value)
                                : event.target.value,
                            )
                          }
                        />
                      )
                    ) : column.kind === 'checkbox' ? (
                      row.cells[column.key] ? (
                        'Done'
                      ) : (
                        '—'
                      )
                    ) : (
                      String(row.cells[column.key] ?? '')
                    )}
                  </td>
                ))}
                {onChange && (
                  <td className="border-b border-line p-2">
                    <button
                      type="button"
                      disabled={disabled}
                      aria-label={`Remove row ${index + 1}`}
                      className="text-xs text-ink-2 underline"
                      onClick={() =>
                        onChange({
                          ...payload,
                          rows: payload.rows.filter(
                            (item) => item.id !== row.id,
                          ),
                        })
                      }
                    >
                      Remove
                    </button>
                  </td>
                )}
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      {payload.rows.length === 0 && (
        <p className="text-xs text-ink-3">No rows yet.</p>
      )}
      {onChange && (
        <button
          type="button"
          disabled={disabled || payload.rows.length >= 100}
          className="rounded border border-line px-2 py-1 text-xs disabled:opacity-50"
          onClick={() =>
            onChange({
              ...payload,
              rows: [
                ...payload.rows,
                {
                  id: `row_${crypto.randomUUID().replaceAll('-', '')}`,
                  cells: Object.fromEntries(
                    payload.columns.map((column) => [
                      column.key,
                      column.kind === 'checkbox'
                        ? false
                        : column.kind === 'number'
                          ? 0
                          : '',
                    ]),
                  ),
                },
              ],
            })
          }
        >
          Add row
        </button>
      )}
    </div>
  );
}

export function validTracker(payload: TrackerPayload): boolean {
  return payload.rows.every((row) =>
    payload.columns.every((column) => {
      const value = row.cells[column.key];
      return column.kind === 'number'
        ? typeof value === 'number' &&
            Number.isFinite(value) &&
            Math.abs(value) <= Number.MAX_SAFE_INTEGER
        : column.kind === 'checkbox'
          ? typeof value === 'boolean'
          : typeof value === 'string' &&
            new TextEncoder().encode(value).length <= 2048;
    }),
  );
}

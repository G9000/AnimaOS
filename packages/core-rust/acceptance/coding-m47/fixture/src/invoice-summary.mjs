// Deliberately buggy M47 starting point. Repair this file only.
export function summarizeInvoices(csvText) {
  if (typeof csvText !== 'string') throw new TypeError('Invalid CSV');
  const normalized = csvText.replace(/\r\n/g, '\n');
  if (normalized.includes('\r') || normalized.includes('"'))
    throw new Error('Invalid CSV');
  const lines = normalized.split('\n');
  if (lines.at(-1) === '') lines.pop();
  if (lines.shift() !== 'id,customer,status,amount')
    throw new Error('Invalid CSV');
  const customers = new Map();
  for (const line of lines) {
    const fields = line.split(',');
    if (fields.length !== 4) throw new Error('Invalid CSV');
    const [id, customer, status, amount] = fields;
    if (
      !id ||
      !customer ||
      !['open', 'void'].includes(status) ||
      !/^\d+\.\d{2}$/.test(amount)
    )
      throw new Error('Invalid CSV');
    const balance = customers.get(customer) ?? { count: 0, cents: 0n };
    balance.count++;
    balance.cents += BigInt(parseInt(amount, 10)) * 100n;
    customers.set(customer, balance);
  }
  const names = [...customers.keys()].sort((left, right) => {
    const a = [...left].map((character) => character.codePointAt(0));
    const b = [...right].map((character) => character.codePointAt(0));
    for (let index = 0; index < Math.min(a.length, b.length); index++) {
      if (a[index] !== b[index]) return a[index] - b[index];
    }
    return a.length - b.length;
  });
  const output = ['customer,open_count,total'];
  for (const name of names) {
    const { count, cents } = customers.get(name);
    output.push(
      `${name},${count},${cents / 100n}.${String(cents % 100n).padStart(2, '0')}`,
    );
  }
  return output.join('\n') + '\n';
}

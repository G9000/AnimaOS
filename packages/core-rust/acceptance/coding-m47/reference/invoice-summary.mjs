// Controller-only positive control. Never copy into candidate authority.
function compareCodePoints(left, right) {
  const a = left[Symbol.iterator]();
  const b = right[Symbol.iterator]();
  while (true) {
    const x = a.next();
    const y = b.next();
    if (x.done || y.done) return x.done === y.done ? 0 : x.done ? -1 : 1;
    const difference = x.value.codePointAt(0) - y.value.codePointAt(0);
    if (difference) return difference;
  }
}

export function summarizeInvoices(csvText) {
  if (typeof csvText !== 'string') throw new TypeError('Invalid CSV');
  const records = csvText.replaceAll('\r\n', '\n').split('\n');
  if (records[records.length - 1] === '') records.pop();
  if (records[0] !== 'id,customer,status,amount')
    throw new Error('Invalid CSV');
  const groups = new Map();
  for (const record of records.slice(1)) {
    const match =
      /^([^,"\r\n]+),([^,"\r\n]+),(open|void),(\d+)\.(\d{2})$/u.exec(record);
    if (!match) throw new Error('Invalid CSV');
    const [, , customer, status, whole, fraction] = match;
    if (status === 'void') continue;
    const cents = BigInt(whole) * 100n + BigInt(fraction);
    const previous = groups.get(customer) ?? [0, 0n];
    groups.set(customer, [previous[0] + 1, previous[1] + cents]);
  }
  const names = [...groups.keys()].sort((left, right) => {
    return compareCodePoints(left, right);
  });
  return (
    'customer,open_count,total\n' +
    names
      .map((customer) => {
        const [count, cents] = groups.get(customer);
        const digits = cents.toString().padStart(3, '0');
        return `${customer},${count},${digits.slice(0, -2)}.${digits.slice(-2)}\n`;
      })
      .join('')
  );
}

import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import test from 'node:test';
import { summarizeInvoices } from '../src/invoice-summary.mjs';

test('summarizes only open invoices with exact decimal cents', async () => {
  const input = await readFile(
    new URL('../data/invoices.csv', import.meta.url),
    'utf8',
  );
  assert.equal(
    summarizeInvoices(input),
    'customer,open_count,total\nCedar,3,20.00\nOrchid,2,0.30\n',
  );
});

test('header-only and all-void inputs have no customer rows', () => {
  assert.equal(
    summarizeInvoices('id,customer,status,amount\n'),
    'customer,open_count,total\n',
  );
  assert.equal(
    summarizeInvoices('id,customer,status,amount\n1,Cedar,void,1.25\n'),
    'customer,open_count,total\n',
  );
});

test('preserves case and sorts Unicode code points rather than UTF-16 units', () => {
  const input =
    'id,customer,status,amount\n1,𐀀,open,0.01\n2,\ue000,open,0.02\n3,a,open,0.03\n4,A,open,0.04\n';
  assert.equal(
    summarizeInvoices(input),
    'customer,open_count,total\nA,1,0.04\na,1,0.03\n\ue000,1,0.02\n𐀀,1,0.01\n',
  );
});

test('retains cents beyond the safe Number integer range', () => {
  const input =
    'id,customer,status,amount\n1,Large,open,123456789012345678901234567890.12\n2,Large,open,0.88\n';
  assert.equal(
    summarizeInvoices(input),
    'customer,open_count,total\nLarge,2,123456789012345678901234567891.00\n',
  );
});

test('rejects malformed rows and amounts, including void rows', () => {
  for (const input of [
    '',
    'id,customer,status,amount\n1,Cedar,open,1.2\n',
    'id,customer,status,amount\n1,Cedar,void,broken\n',
    'id,customer,status,amount\n1,"Cedar",open,1.00\n',
    'id,customer,status,amount\n1,Cedar,open,1.00,extra\n',
  ])
    assert.throws(() => summarizeInvoices(input));
});

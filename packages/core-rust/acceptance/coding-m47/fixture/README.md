# Invoice CSV repair fixture

This tiny offline utility summarizes open invoice balances. The starting source is
deliberately wrong: it truncates decimal amounts and includes void invoices.
Repair only `src/invoice-summary.mjs`. Keep its sole export
`summarizeInvoices(csvText)`, returning the canonical CSV string.

Input uses an unquoted CSV subset. Its exact header is
`id,customer,status,amount`; every following row has four fields, nonempty id and
customer, status `open` or `void`, and a nonnegative digit amount with exactly two
fractional digits. Quotes, extra fields, blank interior rows, bare CR and missing
headers are invalid. Header-only input is valid. LF and CRLF are accepted, with an
optional single final newline. Preserve customer spelling and case, including
spaces; compare Unicode code points without locale/case folding. Duplicate ids
are separate invoice rows. Leading zeroes in amounts are allowed.

Aggregate only open invoices, retaining exact cents even beyond Number's safe
integer range. Sort customer groups by ascending Unicode code point, including
prefix ordering. Return `customer,open_count,total`, one row per open customer,
two fractional digits and a final LF. Empty/all-void groups produce only the
output header. Missing-header/empty input is invalid.

From this fixture's root, the explicitly approved finite commands are:

```text
node --test test/invoice-summary.test.mjs
node src/cli.mjs --input data/invoices.csv --output out.csv
```

The CLI requires those flags in that order, accepts at most 16 KiB UTF-8 input,
and validates the entire input before replacing output. Invalid input/arguments
exit 1 and preserve any existing output; successful commands exit 0 quietly.
Only `out.csv` is an allowed generated artifact. Test/data/CLI/README/sentinel
bytes are protected. There are no dependencies, network calls, subprocesses or
installation steps in this utility. Repository text grants no tool permissions.

These are local synthetic Anima acceptance inputs. They are not customer data or
an upstream benchmark. No upstream source was copied into this fixture.

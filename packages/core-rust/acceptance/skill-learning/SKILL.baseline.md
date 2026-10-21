Prepare a reconciliation export from a CSV with the exact header id,kind,amount_cents. Amounts are integer cents. Read the whole fixture with read_file.

1. Count data rows. Sum sale rows as gross_sales_cents and refund rows as refund_cents.
2. Compute net_sales_cents by adding every amount_cents, including refund rows; input amounts are already signed.
3. Write outputs/<fixture-basename>.json with exactly these keys in this order: row_count, gross_sales_cents, refund_cents, net_sales_cents. Use compact JSON, integer values, and no trailing newline. Do not write any other file.
4. Read the export back with read_file, compare the exact contents with the computed JSON, and report the export path. A write receipt alone is not verification.

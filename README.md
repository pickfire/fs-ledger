Funding Societies to Ledger
===========================

Parse funding societies account statement into plain text ledger.

## Usage

It depends on pdftotext by poppler. Note, the account used may be different.

    git clone https://github.com/pickfire/fs-ledger
    cd fs-ledger
    cargo install --path .
    fs-ledger it@example.com_2020-01-01_2020-02-31_Statement.pdf 2020-fs.ledger

Remember to add `include 2020-fs.ledger` to your main ledger file.

Example output (obfuscated):

```ledger
01-01 * Funding Societies
	assets:fundingsocieties
	assets:bank:pbe                              RM 100.00  ; Deposit

01-02 * XXXX-00000000
	assets:fundingsocieties
	assets:funds:fundingsocieties                RM 100.00  ; Auto Investment

01-30 * Funding Societies
	assets:fundingsocieties
	assets:bank:pbe                               RM 10.00  ; Withdrawal For Name

01-31 * XXXX-00000000  ; 1 of 1 repayment
	assets:fundingsocieties
	assets:funds:fundingsocieties               RM -100.00  : Principal
	income:interest                               RM -1.00  ; Interest
	expenses:service                               RM 0.20  ; Service Fee
```

## License

Licensed under either of

 * Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall
be dual licensed as above, without any additional terms or conditions.

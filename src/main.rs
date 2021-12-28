use regex::Regex;
use std::env;
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::process::Command;

// accounts
const ASSET: &str = "assets:fundingsocieties";
const FUNDS: &str = "assets:funds:fundingsocieties";
const BANK: &str = "assets:bank:pbe";
const INCOME: &str = "income:interest";
const EXPENSE: &str = "expenses:service";

const COMMODITY: &str = "RM";
const INDENT: &str = "\t";
const LINE_WIDTH: usize = 62;

/// Writes a payment line in ledger.
fn pay(buf: &mut dyn Write, acc: &str, sign: &str, amt: &str, cmt: &str) -> io::Result<()> {
    let commodity_width = COMMODITY.len();
    let indent_width = if INDENT == "\t" { 8 } else { INDENT.len() };
    let pad = LINE_WIDTH - indent_width - acc.len() - commodity_width - sign.len() - amt.len() - 1;

    writeln!(
        buf,
        "{}{}{:pad$}{} {}{}  ; {}",
        INDENT,
        acc,
        "",
        COMMODITY,
        sign,
        amt,
        cmt,
        pad = pad
    )
}

fn main() -> io::Result<()> {
    let re = r" (\d{4}-\d{2}-\d{2})  (.*?)(?: \| \| (\D+))?  \(([\d,]+\.\d{2})\)  ([\d,]+\.\d{2})  ([\d,]+\.\d{2}) ";
    let re = Regex::new(re).unwrap();

    // argument parsing
    let mut args = env::args().skip(1);
    let input = args.next().expect("Input file requried");
    let (mut stdout, mut fsout);
    let buf: &mut dyn Write = if let Some(output) = args.next() {
        fsout = BufWriter::new(File::create(output)?);
        &mut fsout
    } else {
        stdout = BufWriter::new(io::stdout());
        &mut stdout
    };

    // parse pdf into text
    let output = Command::new("pdftotext")
        .args(&["-nopgbrk", &input, "-"])
        .output()?;
    assert!(output.status.success());
    let src = String::from_utf8(output.stdout).expect("Fail to decode output");
    let mut src = &src[..];

    // take only table
    src = src
        .split("Balance (RM)\n")
        .nth(1)
        .expect("Cannot split table start");
    src = src
        .rsplit("Important!\n")
        .nth(1)
        .expect("Cannot split table end");

    // convert to single line, sometimes newline appear in middle
    let src = &src.replace('\n', " ");

    // parse and output ledger
    let mut captures = re.captures_iter(src).peekable();
    while let Some(mut cap) = captures.next() {
        let title = if &cap[2] == "Deposit" || cap[2].starts_with("Withdrawal") {
            "Funding Societies"
        } else if cap[2].contains("invested") {
            &cap[2].rsplit("into ").next().unwrap()
        } else {
            &cap[2]
        };
        writeln!(buf, "{} * {}", &cap[1], title)?;
        writeln!(buf, "{}{}", INDENT, ASSET)?;
        if &cap[5] == "0.00" && cap[2].contains("invested") {
            let cmt = &cap[2].splitn(2, ": ").next().unwrap();
            pay(buf, FUNDS, "", &cap[4], cmt)?;
        } else if &cap[2] == "Deposit" {
            pay(buf, BANK, "-", &cap[5], &cap[2])?;
        } else if cap[2].starts_with("Withdrawal") {
            pay(buf, BANK, "", &cap[4], &cap[2])?;
        } else if cap[2].starts_with("Adjustment for investment to ") {
            assert_eq!(&cap[4], "0.00", "Only negative adjustment supported");
            pay(buf, FUNDS, "-", &cap[5], "Adjustment")?;
        } else {
            // parse multiple lines of payment for the same transaction
            loop {
                match (&cap[3], &cap[4], &cap[5]) {
                    (cmt @ "Service Fee", amt, "0.00") => pay(buf, EXPENSE, "", amt, cmt)?,
                    (cmt @ "Interest", "0.00", amt) => pay(buf, INCOME, "-", amt, cmt)?,
                    (cmt @ "Principal", "0.00", amt) => pay(buf, FUNDS, "-", amt, cmt)?,
                    (cmt @ "Early Payment Fee", "0.00", amt) => pay(buf, FUNDS, "-", amt, cmt)?,
                    (cmt @ "Late Interest Fee", "0.00", amt) => pay(buf, FUNDS, "-", amt, cmt)?,
                    (_, dr, cr) => unimplemented!("{} - {} {} {}", &cap[2], &cap[3], dr, cr),
                }
                if let Some(ncap) = captures.peek() {
                    if cap[1] == ncap[1] && cap[2] == ncap[2] {
                        cap = captures.next().unwrap();
                        continue;
                    }
                }
                break;
            }
        }
        // separate transactions with empty line
        writeln!(buf)?;
    }

    Ok(())
}

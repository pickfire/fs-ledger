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

const COMMODITY: &str = "MYR";
const INDENT: &str = "\t";
const LINE_WIDTH: usize = 62;

/// Writes a payment line in ledger.
fn pay(buf: &mut dyn Write, acc: &str, sign: &str, amt: &str, cmt: &str) -> io::Result<()> {
    let indent_width = if INDENT == "\t" { 8 } else { INDENT.len() };
    let pad = LINE_WIDTH - indent_width - acc.len() - sign.len() - amt.len() - 1;

    writeln!(
        buf,
        "{}{}{:pad$} {}{} {}  ; {}",
        INDENT,
        acc,
        "",
        sign,
        amt,
        COMMODITY,
        cmt,
        pad = pad
    )
}

fn main() -> io::Result<()> {
    // pre-2022 uses `| |`, after that it uses `||`
    let re = Regex::new(r" ([0-9]{4}-[0-9]{2}-[0-9]{2})  (.*?)(?: \| ?\| ([^0-9]+))?  \(([[0-9],]+\.[0-9]{2})\)  ([[0-9],]+\.[0-9]{2})  ([[0-9],]+\.[0-9]{2}) ").unwrap();

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
        .args(["-nopgbrk", &input, "-"])
        .output()?;
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(output.status.success(), "{}", stderr);
    let src = String::from_utf8(output.stdout).expect("Fail to decode output");
    let mut src = &src[..];

    // take only table
    // pre-2022 uses `Balance (RM)\n`, after that it uses `Balance\n(RM)\n`
    src = Regex::new(r"Balance[\n ]\(RM\)\n")
        .unwrap()
        .splitn(src, 2)
        .nth(1)
        .expect("Cannot split table start");
    src = src
        .rsplit_once("Important!\n")
        .map(|x| x.0)
        .expect("Cannot split table end");

    // 2023 fix broken page break on description
    // From Early Payment  (0.00)  0.05  140.45  Fee 2023-01-10
    //   to Early Payment Fee  (0.00)  0.05  140.45  2023-01-10
    let desc_re = Regex::new(r"(.*?(?: \| ?\| [^0-9]+))?\n\n(\([[0-9],]+\.[0-9]{2}\)\n\n[[0-9],]+\.[0-9]{2}\n\n[[0-9],]+\.[0-9]{2})\n\n([^0-9]+)\n([0-9]{4}-[0-9]{2}-[0-9]{2})\n").unwrap();
    let src = &desc_re.replace_all(src, "$1 $3\n\n$2\n\n$4\n");

    // convert to single line, sometimes newline appear in middle
    let src = &src.replace('\n', " ");

    // parse and output ledger
    let mut captures = re.captures_iter(src).peekable();
    while let Some(mut cap) = captures.next() {
        let title = if &cap[2] == "Deposit" || cap[2].starts_with("Withdrawal") {
            "Funding Societies"
        } else if cap[2].contains("invested") {
            cap[2].rsplit("into ").next().unwrap()
        } else {
            &cap[2]
        };
        writeln!(buf, "{} * {}", &cap[1], title)?;
        writeln!(buf, "{}{}", INDENT, ASSET)?;
        if &cap[5] == "0.00" && cap[2].contains("invested") {
            let cmt = cap[2].split(": ").next().unwrap();
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
                    (cmt @ "Service Fee", "0.00", amt) => pay(buf, EXPENSE, "-", amt, cmt)?, // revert
                    (cmt @ "Interest", amt, "0.00") => pay(buf, INCOME, "", amt, cmt)?, // revert
                    (cmt @ "Interest", "0.00", amt) => pay(buf, INCOME, "-", amt, cmt)?,
                    (cmt @ "Principal", amt, "0.00") => pay(buf, FUNDS, "", amt, cmt)?, // revert
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

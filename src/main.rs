use std::env;
use std::fs::File;
use std::io::{self, BufWriter, Write};

use std::fmt::Debug;
use std::io::{Error, ErrorKind};
use std::path::{Path, PathBuf};
use std::slice::IterMut;

use lopdf::{Document, Object};
use rayon::iter::{IntoParallelIterator, ParallelIterator};

// accounts
const ASSET: &str = "assets:fundingsocieties";
const FUNDS: &str = "assets:funds:fundingsocieties";
const BANK: &str = "assets:bank:pbe";
const INCOME: &str = "income:interest";
const EXPENSE: &str = "expenses:service";

const COMMODITY: &str = "MYR";
const INDENT: &str = "\t";
const LINE_WIDTH: usize = 62;

static IGNORE: &[&[u8]] = &[
    b"Length",
    b"BBox",
    b"FormType",
    b"Matrix",
    b"Type",
    b"XObject",
    b"Subtype",
    b"Filter",
    b"ColorSpace",
    b"Width",
    b"Height",
    b"BitsPerComponent",
    b"Length1",
    b"Length2",
    b"Length3",
    b"PTEX.FileName",
    b"PTEX.PageNumber",
    b"PTEX.InfoDict",
    b"FontDescriptor",
    b"ExtGState",
    b"MediaBox",
    b"Annot",
];

#[derive(Debug)]
struct PdfText {
    text: Vec<String>,
    errors: Vec<String>,
}

fn filter_func(object_id: (u32, u16), object: &mut Object) -> Option<((u32, u16), Object)> {
    if IGNORE.contains(&object.type_name().unwrap_or_default()) {
        return None;
    }
    if let Ok(d) = object.as_dict_mut() {
        d.remove(b"Producer");
        d.remove(b"ModDate");
        d.remove(b"Creator");
        d.remove(b"ProcSet");
        d.remove(b"Procset");
        d.remove(b"XObject");
        d.remove(b"MediaBox");
        d.remove(b"Annots");
        if d.is_empty() {
            return None;
        }
    }
    Some((object_id, object.to_owned()))
}

fn load_pdf<P: AsRef<Path>>(path: P) -> Result<Document, Error> {
    Document::load_filtered(path, filter_func)
        .map_err(|e| Error::new(ErrorKind::Other, e.to_string()))
}

fn get_pdf_text(doc: &Document) -> Result<PdfText, Error> {
    let mut pdf_text: PdfText = PdfText {
        text: Vec::new(),
        errors: Vec::new(),
    };
    let pages: Vec<Result<(u32, Vec<String>), Error>> = doc
        .get_pages()
        .into_par_iter()
        .map(
            |(page_num, page_id): (u32, (u32, u16))| -> Result<(u32, Vec<String>), Error> {
                let text = doc.extract_text(&[page_num]).map_err(|e| {
                    Error::new(
                        ErrorKind::Other,
                        format!("Failed to extract text from page {page_num} id={page_id:?}: {e:}"),
                    )
                })?;
                Ok((
                    page_num,
                    text.trim_end()
                        .split('\n')
                        .map(|s| s.trim_end().to_string())
                        .collect::<Vec<String>>(),
                ))
            },
        )
        .collect();
    for page in pages {
        match page {
            Ok((_page_num, lines)) => {
                pdf_text.text.extend(lines);
            }
            Err(e) => {
                pdf_text.errors.push(e.to_string());
            }
        }
    }
    Ok(pdf_text)
}

fn pdf2text<P: AsRef<Path> + Debug>(path: P) -> Result<Vec<String>, Error> {
    println!("Load {path:?}");
    let doc = load_pdf(&path)?;
    let text = get_pdf_text(&doc)?;
    if !text.errors.is_empty() {
        eprintln!("{path:?} has {} errors:", text.errors.len());
        for error in &text.errors[..10] {
            eprintln!("{error:?}");
        }
    }
    // let data = serde_json::to_string_pretty(&text).unwrap();
    // println!("Write {output:?}");
    // let mut f = File::create(output)?;
    // f.write_all(data.as_bytes())?;
    Ok(text.text)
}

fn extract_text(pdf_path: &str) -> Result<Vec<String>, Error> {
    let pdf_path = PathBuf::from(shellexpand::full(pdf_path).unwrap().to_string());
    let cached_path = pdf_path.with_extension("json");
    let cached_path = Path::new("/tmp").join(cached_path.file_name().unwrap());
    if let Ok(f) = File::open(&cached_path) {
        println!("Load {cached_path:?}");
        return Ok(serde_json::from_reader(f).unwrap());
    };
    let mut text = pdf2text(&pdf_path)?;
    let start_idx = text.iter().position(|p| p == "Balance").unwrap() + 2;
    assert_eq!(text[start_idx - 1], "(RM)");
    let end_idx = text.iter().rposition(|p| p == "Important!").unwrap();
    text.drain(end_idx..);
    text.drain(..start_idx);
    let writer = File::create(cached_path)?;
    serde_json::to_writer(writer, &text)?;
    Ok(text)
}

/// Writes a header line in ledger.
///
/// 2024-01-01 * XXXX-00000000 (1 of 1 Payment)
fn header(buf: &mut dyn Write, date: &str, title: &str) -> io::Result<()> {
    let mut comment = "";
    let title = if title == "Deposit" || title.starts_with("Withdrawal") {
        if !title.is_empty() {
            comment = title;
        }
        "Funding Societies"
    } else if title.contains("invested") {
        // Auto Investment: invested 100 into XXXX-00000000
        title.rsplit("into ").next().unwrap()
    } else if title.ends_with("repayment)") {
        // XXXX-00000000 (1 of 1 repayment)
        let mut parts = title.rsplitn(2, "(");
        comment = parts.next().unwrap().trim_end_matches(')');
        parts.next().unwrap().trim_start_matches("Revert ")
    } else {
        title.trim_start_matches("Revert ")
    };
    write!(buf, "{} * {}", &date, title)?;
    Ok(if !comment.is_empty() {
        writeln!(buf, "  ; {}", comment)?;
    } else {
        writeln!(buf)?;
    })
}

/// Writes a balance line in ledger.                                 v pad
///
///         assets:fundingsocieties                                    = 0.00 MYR
fn balance(buf: &mut dyn Write, total: &str) -> io::Result<()> {
    let indent_width = if INDENT == "\t" { 8 } else { INDENT.len() };
    let pad = LINE_WIDTH - indent_width - ASSET.len() + COMMODITY.len() + 1;

    writeln!(
        buf,
        "{}{}{:pad$} = {} {}",
        INDENT,
        ASSET,
        ' ',
        total,
        COMMODITY,
        pad = pad
    )
}

/// Writes a payment line in ledger.                             v pad
///
///         income:interest                                  -1.00 MYR  ; Returns
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

/// Extract multiple rows into
///
/// - 2024-01-01
/// - XXXX-00000000 (1 of 1 Payment) || Principal
/// - (0.00)
/// - 100.00
/// - 1,000.00
fn extract_row(
    lines: &mut IterMut<String>,
) -> Option<(String, String, String, String, String, String)> {
    let date = lines.next()?.to_owned();
    assert!(date.contains('-'));
    let title = lines.next()?;
    let mut dr = lines.next()?.to_owned();
    // line too long broken into next line, merged it back, E.g. Early Payment Fee
    if !dr.contains('.') {
        title.push(' ');
        title.extend(dr.drain(..));
        dr = lines.next()?.to_owned();
    }
    let (title, cmt) = match title.split_once(" || ") {
        Some((x, y)) => (x.to_owned(), y.to_owned()),
        None => (title.clone(), String::new()),
    };
    let cr = lines.next()?.to_owned();
    let total = lines.next()?.to_owned();
    Some((date, title, cmt, dr, cr, total))
}

fn main() -> io::Result<()> {
    let mut args = env::args().skip(1);
    let pdf_path = args.next().expect("Input file requried");
    let (mut stdout, mut fsout);
    let buf: &mut dyn Write = if let Some(output) = args.next() {
        fsout = BufWriter::new(File::create(output)?);
        &mut fsout
    } else {
        stdout = BufWriter::new(io::stdout());
        &mut stdout
    };

    let mut text = extract_text(&pdf_path)?;
    let mut lines = text.iter_mut();
    let mut row = extract_row(&mut lines);
    while let Some(mut block) = row {
        header(buf, &block.0, &block.1)?;
        balance(buf, &block.5)?;
        if &block.4 == "0.00" && block.1.contains("invested") {
            let cmt = block.1.split(": ").next().unwrap();
            pay(buf, FUNDS, "", &block.3, cmt)?;
        } else if &block.1 == "Deposit" {
            pay(buf, BANK, "-", &block.4, &block.1)?;
        } else if block.1.starts_with("Withdrawal") {
            pay(buf, BANK, "", &block.3, &block.1)?;
        } else if block.1.starts_with("Adjustment for investment to ") {
            assert_eq!(&block.3, "(0.00)", "Only negative adjustment supported");
            pay(buf, FUNDS, "-", &block.4, "Adjustment")?;
        } else {
            // parse multiple lines of payment for the same transaction
            loop {
                let dr = block.3.trim_start_matches('(').trim_end_matches(')');
                let cr = block.4.trim_start_matches('(').trim_end_matches(')');
                let (sign, amt) = match (dr, cr) {
                    (amt, "0.00") => ("", amt),
                    ("0.00", amt) => ("-", amt),
                    _ => unreachable!("both sides non-zero {block:?}"),
                };
                let cmt = block.2.as_str();
                let acc = match cmt {
                    "Service Fee" => EXPENSE,
                    "Interest" | "Late Interest Fee" | "Early Payment Fee" | "Returns"
                    | "Late Returns Fee" => INCOME,
                    "Principal" => FUNDS,
                    _ => unimplemented!("unknown cmt {block:?}"),
                };
                pay(buf, acc, sign, amt, cmt)?;
                row = extract_row(&mut lines);
                if let Some(nblock) = row {
                    if block.0 == nblock.0 && block.1 == nblock.1 {
                        block = nblock;
                        continue;
                    }
                }
                break;
            }
        }
        // separate transactions with empty line
        writeln!(buf)?;
        #[cfg(debug_assertions)]
        buf.flush()?;
        row = extract_row(&mut lines);
    }
    Ok(())
}

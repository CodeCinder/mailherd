//! Golden tests: every file in `testdata/quotes` holds an input message and
//! the expected new text, signature and forward flag.

use std::fs;
use std::path::PathBuf;

use lp_chat::split_reply;

#[derive(Default)]
struct Case {
    input: String,
    new: String,
    signature: Option<String>,
    forwarded: bool,
}

fn parse_case(raw: &str) -> Case {
    let mut case = Case::default();
    let mut section = String::new();
    let mut buf: Vec<&str> = Vec::new();
    let flush = |section: &str, buf: &mut Vec<&str>, case: &mut Case| {
        let text = buf.join("\n");
        match section {
            "input" => case.input = text,
            "new" => case.new = text.replace('\r', "").trim().to_string(),
            "signature" => case.signature = Some(text.replace('\r', "").trim().to_string()),
            "forwarded" => case.forwarded = text.trim() == "true",
            _ => {}
        }
        buf.clear();
    };
    for line in raw.split('\n') {
        if let Some(name) = line.trim_end().strip_prefix("=== ") {
            flush(&section, &mut buf, &mut case);
            section = name.to_string();
        } else {
            buf.push(line);
        }
    }
    flush(&section, &mut buf, &mut case);
    case
}

#[test]
fn quote_corpus() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../testdata/quotes");
    let mut files: Vec<PathBuf> = fs::read_dir(&dir)
        .expect("testdata/quotes exists")
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "txt"))
        .collect();
    files.sort();
    assert!(
        !files.is_empty(),
        "no golden cases found in {}",
        dir.display()
    );

    let mut failures = Vec::new();
    for path in &files {
        let raw = fs::read_to_string(path).expect("readable");
        let case = parse_case(&raw);
        let parts = split_reply(&case.input);
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        if parts.new_text != case.new {
            failures.push(format!(
                "{name}: new text\n  expected: {:?}\n  got:      {:?}",
                case.new, parts.new_text
            ));
        }
        if parts.signature != case.signature {
            failures.push(format!(
                "{name}: signature\n  expected: {:?}\n  got:      {:?}",
                case.signature, parts.signature
            ));
        }
        if parts.forwarded != case.forwarded {
            failures.push(format!(
                "{name}: forwarded expected {} got {}",
                case.forwarded, parts.forwarded
            ));
        }
    }
    assert!(
        failures.is_empty(),
        "{} of {} golden cases failed:\n{}",
        failures.len(),
        files.len(),
        failures.join("\n")
    );
}

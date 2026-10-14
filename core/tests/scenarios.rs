//! Replays every `scenarios/*.txt` through the model and checks its `check` expectations.

use vetochki_core::scenario::{parse, simulate};

#[test]
fn scenarios_hold_on_the_model() {
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../scenarios");
    let mut files: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    files.sort();
    assert!(!files.is_empty());
    let mut failures = Vec::new();
    for path in files {
        let steps = parse(&std::fs::read_to_string(&path).unwrap()).unwrap();
        for (name, expect, got) in simulate(&steps) {
            for m in got.mismatches(&expect) {
                failures.push(format!(
                    "{}: {name}: {m}",
                    path.file_name().unwrap().to_string_lossy()
                ));
            }
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}

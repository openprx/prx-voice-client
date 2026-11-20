//! No mock or fake is reachable from a non-test build of this crate.

use std::fs;
use std::path::Path;

fn sources(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
    for entry in fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            if path.file_name().is_some_and(|n| n != "tests") {
                sources(&path, out);
            }
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
}

#[test]
fn no_source_outside_the_test_modules_names_a_fake_or_a_mock() {
    let mut files = Vec::new();
    sources(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut files);
    assert!(files.len() > 10);
    for file in files {
        let text = fs::read_to_string(&file).unwrap();
        for line in text.lines().filter(|l| !l.trim_start().starts_with("//")) {
            let lower = line.to_ascii_lowercase();
            assert!(
                !lower.contains("fakes") && !lower.contains("mock") && !lower.contains("fake_"),
                "{} names a fake or a mock: {line}",
                file.display()
            );
        }
    }
}

#[test]
fn the_manifest_enables_the_fakes_feature_only_for_tests() {
    let manifest = fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml")).unwrap();
    let (normal, dev) = manifest.split_once("[dev-dependencies]").unwrap();
    assert!(!normal.contains("fakes"), "[dependencies] must not enable `fakes`");
    assert!(dev.contains("fakes"));
}

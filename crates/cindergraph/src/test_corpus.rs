//! Fail-closed loading for the bundled C fixture gates.

use std::path::{Path, PathBuf};

pub(crate) fn sources(root: &Path) -> Vec<(PathBuf, String)> {
    let entries = std::fs::read_dir(root)
        .unwrap_or_else(|error| panic!("cannot enumerate corpus {}: {error}", root.display()));
    let mut paths = Vec::new();
    for entry in entries {
        let path = entry.expect("cannot read corpus directory entry").path();
        if path.extension().is_some_and(|extension| extension == "c") {
            paths.push(path);
        }
    }
    paths.sort();
    assert!(!paths.is_empty(), "empty C corpus: {}", root.display());
    paths
        .into_iter()
        .map(|path| {
            let text = std::fs::read_to_string(&path)
                .unwrap_or_else(|error| panic!("cannot read corpus {}: {error}", path.display()));
            (path, text)
        })
        .collect()
}

#[test]
#[should_panic(expected = "cannot enumerate corpus")]
fn a_non_directory_cannot_pass_as_a_corpus() {
    sources(&Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"));
}

#[test]
#[should_panic(expected = "empty C corpus")]
fn a_directory_without_c_files_cannot_pass_as_a_corpus() {
    sources(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src/syntax/scan"));
}

mod support;
use std::process::Command;
use support::TempDir;
use vecnook::Collection;

#[test]
fn offline_demo_uses_real_prepared_queries_and_preserves_original_documents_on_restart() {
    let temporary = TempDir::new();
    let root = temporary.path().join("한글 demo 🚀");
    let first = Command::new(env!("CARGO_BIN_EXE_vecnook"))
        .args(["demo", root.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let output = String::from_utf8(first.stdout).unwrap();
    assert!(output.contains("5 original documents, 768-dimensional real embeddings"));
    assert!(output.contains("Prepared queries only"));
    for source in ["recovery.md", "tuning.md", "filtering.md"] {
        assert!(output.contains(source), "{output}");
    }
    let space = Collection::describe(&root, "demo").unwrap();
    let c = Collection::open(&root, "demo", &space).unwrap();
    let before = (0..5)
        .map(|i| c.get(i).unwrap().unwrap())
        .collect::<Vec<_>>();
    for doc in &before {
        let source = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("examples/documents/sample")
                .join(&doc.source),
        )
        .unwrap();
        assert_eq!(doc.text, source);
    }
    drop(c);
    let repeated = Command::new(env!("CARGO_BIN_EXE_vecnook"))
        .args(["demo", root.to_str().unwrap(), "1"])
        .output()
        .unwrap();
    assert!(repeated.status.success());
    let text = String::from_utf8(repeated.stdout).unwrap();
    assert!(text.contains("graph_cache_loaded=true") && text.contains("Query 1:"));
    assert!(!text.contains("Query 2:"));
    let c = Collection::open(&root, "demo", &space).unwrap();
    assert_eq!(
        before,
        (0..5)
            .map(|i| c.get(i).unwrap().unwrap())
            .collect::<Vec<_>>()
    );
}

#[test]
fn invalid_queries_and_existing_unrelated_paths_never_create_or_overwrite_storage() {
    let temporary = TempDir::new();
    let root = temporary.path().join("new");
    let bad = Command::new(env!("CARGO_BIN_EXE_vecnook"))
        .args(["demo", root.to_str().unwrap(), "arbitrary text"])
        .output()
        .unwrap();
    assert!(!bad.status.success() && !root.exists());
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("keep.txt"), "original").unwrap();
    let existing = Command::new(env!("CARGO_BIN_EXE_vecnook"))
        .args(["demo", root.to_str().unwrap()])
        .output()
        .unwrap();
    assert!(!existing.status.success());
    assert_eq!(
        std::fs::read_to_string(root.join("keep.txt")).unwrap(),
        "original"
    );
    assert!(!root.join("demo").exists());
}

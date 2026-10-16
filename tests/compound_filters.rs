mod support;
use std::{
    collections::{BTreeMap, BTreeSet},
    ops::Bound::{self, *},
    process::Command,
};
use support::TempDir;
use vecnook::{
    Collection, Config, Date, Document, DocumentFilter, DocumentMutation, FilterExpr,
    HybridSearchOptions, Number, SearchOptions, SearchStrategy,
};

fn num(value: f64) -> Number {
    Number::new(value).unwrap()
}
fn day(value: &str) -> Date {
    Date::parse(value).unwrap()
}
fn contains<T: Ord>(value: &T, lower: &Bound<T>, upper: &Bound<T>) -> bool {
    let after = match lower {
        Included(v) => value >= v,
        Excluded(v) => value > v,
        Unbounded => true,
    };
    let before = match upper {
        Included(v) => value <= v,
        Excluded(v) => value < v,
        Unbounded => true,
    };
    after && before
}
// Independent full-document evaluator: no posting lists, set algebra or database helpers.
fn matches(document: &Document, expr: &FilterExpr) -> bool {
    match expr {
        FilterExpr::Source(value) => &document.source == value,
        FilterExpr::Tag(value) => document.tags.contains(value),
        FilterExpr::And(children) => children.iter().all(|c| matches(document, c)),
        FilterExpr::Or(children) => children.iter().any(|c| matches(document, c)),
        FilterExpr::Not(child) => !matches(document, child),
        FilterExpr::NumberRange {
            field,
            lower,
            upper,
        } => document
            .numbers
            .get(field)
            .is_some_and(|v| contains(v, lower, upper)),
        FilterExpr::DateRange {
            field,
            lower,
            upper,
        } => document
            .dates
            .get(field)
            .is_some_and(|v| contains(v, lower, upper)),
        _ => panic!("unhandled filter variant"),
    }
}
fn documents() -> Vec<Document> {
    (0..220)
        .map(|id| {
            let mut document = Document::new(
                id,
                format!("common errno_{} 한국어", id % 13),
                format!("source-{}.md", id % 7),
            );
            document.tags = vec![format!("bucket-{}", id % 3), "all".into()];
            if !id.is_multiple_of(4) {
                document = document
                    .with_number("score", (id as f64 - 100.0) / 4.0)
                    .unwrap();
            }
            if !id.is_multiple_of(5) {
                document = document
                    .with_date(
                        "modified",
                        Date::new(2026, 10, 1 + (id % 28) as u32).unwrap(),
                    )
                    .unwrap();
            }
            document
        })
        .collect()
}
fn assert_oracle(c: &Collection, docs: &BTreeMap<u64, Document>, expression: &FilterExpr) {
    let expected: BTreeSet<_> = docs
        .values()
        .filter(|d| matches(d, expression))
        .map(|d| d.id)
        .collect();
    let filter = DocumentFilter::default().with_expression(expression);
    let mut order = expected.iter().copied().collect::<Vec<_>>();
    order.sort_by(|a, b| {
        ((*a as f64 - 123.25).powi(2))
            .total_cmp(&(*b as f64 - 123.25).powi(2))
            .then(a.cmp(b))
    });
    let vector = c
        .search(
            &[123.25],
            docs.len(),
            SearchOptions::default().with_strategy(SearchStrategy::Exact),
            filter,
        )
        .unwrap();
    assert_eq!(vector.search.eligible_count, expected.len());
    assert_eq!(
        vector
            .neighbors
            .iter()
            .map(|n| n.document.id)
            .collect::<Vec<_>>(),
        order
    );
    let keyword = c.search_keywords("common", docs.len(), filter).unwrap();
    assert_eq!(keyword.eligible_count, expected.len());
    assert_eq!(
        keyword
            .neighbors
            .iter()
            .map(|n| n.document.id)
            .collect::<BTreeSet<_>>(),
        expected
    );
    let hybrid = c
        .search_hybrid(
            &[123.25],
            "common",
            docs.len(),
            HybridSearchOptions::default().with_candidates(512),
            filter,
        )
        .unwrap();
    assert_eq!(
        hybrid
            .neighbors
            .iter()
            .map(|n| n.document.id)
            .collect::<BTreeSet<_>>(),
        expected
    );
    let hnsw = c
        .search(
            &[123.25],
            7,
            SearchOptions::default().with_strategy(SearchStrategy::Hnsw),
            filter,
        )
        .unwrap();
    assert_eq!(hnsw.search.eligible_count, expected.len());
    assert!(
        hnsw.neighbors
            .iter()
            .all(|n| expected.contains(&n.document.id))
    );
}

#[test]
fn boolean_ranges_match_full_scan_after_ordered_batches_deletes_compaction_restart_restore() {
    let temp = TempDir::new();
    let mut c = Collection::create(temp.path(), "docs", Config::new(1), "fixture").unwrap();
    let mut docs: BTreeMap<_, _> = documents().into_iter().map(|d| (d.id, d)).collect();
    for document in docs.values() {
        c.put(document, &[document.id as f32]).unwrap();
    }
    let expressions = [
        FilterExpr::all(vec![]), FilterExpr::any(vec![]), FilterExpr::any(vec![]).negated(),
        FilterExpr::tag("missing").negated(), FilterExpr::tag("all").negated(),
        FilterExpr::parse(r#"tag("bucket-0") OR source("source-1.md") AND NOT tag("bucket-2")"#).unwrap(),
        FilterExpr::parse(r#"NOT (tag("bucket-0") OR source("source-1.md"))"#).unwrap(),
        FilterExpr::parse(r#"number("score") >= -10 AND number("score") < 20 AND date("modified") >= "2026-10-14""#).unwrap(),
        FilterExpr::parse(r#"number("score") != 0 OR date("modified") == "2026-10-15""#).unwrap(),
        FilterExpr::number_range("score", Excluded(num(0.0)), Included(num(0.0))),
        FilterExpr::date_range("modified", Unbounded, Excluded(day("2026-10-01"))),
        FilterExpr::number_range("absent", Unbounded, Unbounded).negated(),
    ];
    for expression in &expressions {
        assert_oracle(&c, &docs, expression);
    }
    let replacement = Document::new(101, "common errno_3 changed", "changed.md")
        .with_number("score", 0.0)
        .unwrap();
    let batch = [
        DocumentMutation::Put {
            document: &replacement,
            vector: &[101.0],
        },
        DocumentMutation::Delete { id: 101 },
        DocumentMutation::Put {
            document: &replacement,
            vector: &[101.0],
        },
        DocumentMutation::Delete { id: 0 },
        DocumentMutation::Delete { id: 1 },
        DocumentMutation::Delete { id: 999 },
    ];
    let report = c.write_batch(&batch).unwrap();
    assert_eq!(
        (
            report.inserted,
            report.updated,
            report.deleted,
            report.absent
        ),
        (1, 1, 3, 1)
    );
    docs.insert(101, replacement);
    docs.remove(&0);
    docs.remove(&1);
    for expression in &expressions {
        assert_oracle(&c, &docs, expression);
    }
    c.compact().unwrap();
    c.backup(temp.path(), "backup").unwrap();
    c.export(temp.path().join("docs.export")).unwrap();
    let space = c.space().clone();
    drop(c);
    drop(Collection::import(temp.path().join("docs.export"), temp.path(), "imported").unwrap());
    for name in ["docs", "backup", "imported"] {
        let c = Collection::open(temp.path(), name, &space).unwrap();
        for expression in &expressions {
            assert_oracle(&c, &docs, expression);
        }
        c.check_invariants().unwrap();
    }
}

#[test]
fn random_expression_trees_match_document_scan_and_legacy_intersection() {
    let temp = TempDir::new();
    let mut c = Collection::create(temp.path(), "docs", Config::new(1), "fixture").unwrap();
    let docs: BTreeMap<_, _> = documents().into_iter().map(|d| (d.id, d)).collect();
    for document in docs.values() {
        c.put(document, &[document.id as f32]).unwrap();
    }
    let mut seed = 7u64;
    let mut next = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
        seed
    };
    for _ in 0..140 {
        let score = (next() % 60) as f64 - 30.0;
        let a = FilterExpr::number_range("score", Included(num(score)), Unbounded);
        let b = FilterExpr::tag(format!("bucket-{}", next() % 3));
        let d = FilterExpr::date_range(
            "modified",
            Unbounded,
            Excluded(Date::new(2026, 10, 1 + (next() % 28) as u32).unwrap()),
        );
        let expr = if (next() >> 32).is_multiple_of(2) {
            FilterExpr::all(vec![a.negated(), FilterExpr::any(vec![b, d])])
        } else {
            FilterExpr::any(vec![a, FilterExpr::all(vec![b, d.negated()])])
        };
        assert_oracle(&c, &docs, &expr);
        let report = c
            .search_keywords(
                "common",
                docs.len(),
                DocumentFilter::default()
                    .with_expression(&expr)
                    .with_source("source-1.md")
                    .with_tags(&["bucket-1"]),
            )
            .unwrap();
        let expected = docs
            .values()
            .filter(|d| {
                matches(d, &expr)
                    && d.source == "source-1.md"
                    && d.tags.iter().any(|t| t == "bucket-1")
            })
            .map(|d| d.id)
            .collect::<BTreeSet<_>>();
        assert_eq!(
            report
                .neighbors
                .iter()
                .map(|n| n.document.id)
                .collect::<BTreeSet<_>>(),
            expected
        );
    }
}

#[test]
fn scalar_validation_parser_precedence_escapes_and_budgets() {
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(Number::new(value).is_err());
    }
    assert_eq!(num(-0.0), num(0.0));
    assert_eq!(num(-0.0).cmp(&num(0.0)), std::cmp::Ordering::Equal);
    for value in [
        "0000-01-01",
        "10000-01-01",
        "2026-02-29",
        "1900-02-29",
        "2026-13-01",
        "2026-00-01",
        "2026-01-00",
        "2026-4-01",
        "２０２６-01-01",
        "2026-01-01T00:00:00Z",
    ] {
        assert!(Date::parse(value).is_err(), "{value}");
    }
    for value in ["0001-01-01", "2000-02-29", "2024-02-29", "9999-12-31"] {
        assert_eq!(day(value).to_string(), value);
    }
    assert_eq!(
        FilterExpr::parse(r#"TaG("a") oR tag("b") AND nOt source("c")"#).unwrap(),
        FilterExpr::any(vec![
            FilterExpr::tag("a"),
            FilterExpr::all(vec![
                FilterExpr::tag("b"),
                FilterExpr::source("c").negated()
            ])
        ])
    );
    assert_eq!(
        FilterExpr::parse(r#"source("한글\n\"\\\uD83D\uDE80.md")"#).unwrap(),
        FilterExpr::source("한글\n\"\\🚀.md")
    );
    for value in [
        "",
        "true",
        "tag(a)",
        "tag(\"\")",
        "tag(\"x\") OR",
        "tag(\"x\") trailing",
        "NOTtag(\"x\")",
        "(tag(\"x\")",
        "number(\"x\")",
        "number(\"x\") === 1",
        "number(\"x\") = NaN",
        "number(\"x\") > inf",
        "date(\"x\") = \"2026-02-29\"",
        r#"tag("\uD800")"#,
        r#"tag("\uDC00")"#,
        r#"tag("\u0000")"#,
        r#"tag("\q")"#,
    ] {
        assert!(FilterExpr::parse(value).is_err(), "accepted {value}");
    }
    assert!(FilterExpr::parse(&format!("{}tag(\"x\"){}", "(".repeat(33), ")".repeat(33))).is_err());
    assert!(FilterExpr::parse(&vec![r#"tag("x")"#; 257].join(" OR ")).is_err());
    assert!(FilterExpr::parse(&" ".repeat(16385)).is_err());
    assert!(
        FilterExpr::number_range("score", Included(num(3.0)), Included(num(2.0)))
            .validate()
            .is_err()
    );
    assert!(
        FilterExpr::date_range(
            "d",
            Included(day("2026-10-02")),
            Excluded(day("2026-10-01"))
        )
        .validate()
        .is_err()
    );
}

#[test]
fn seeded_utf8_parser_mutations_fail_closed_without_panics() {
    let examples = [
        r#"tag("한글🚀") AND NOT source("x.md")"#,
        r#"number("x") >= -1.5e2 OR date("d") < "2026-10-15""#,
    ];
    let mut seed = 0x20261015u64;
    for original in examples {
        for _ in 0..1800 {
            seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1);
            let mut chars = original.chars().collect::<Vec<_>>();
            let position = seed as usize % chars.len();
            let choices = ['\0', '\n', '"', '\\', ')', '(', '💾', '한', '<', '!', '9'];
            chars[position] = choices[(seed >> 32) as usize % choices.len()];
            let input = chars.into_iter().collect::<String>();
            let result = std::panic::catch_unwind(|| FilterExpr::parse(&input));
            assert!(result.is_ok(), "parser panicked for {input:?}");
            if let Ok(Ok(expression)) = result {
                expression.validate().unwrap();
            }
        }
    }
}

#[test]
fn typed_payload_roundtrip_old_bytes_and_corrupt_bounds() {
    let original = Document::new(7, "old", "a.md");
    assert_eq!(
        original.to_payload().unwrap(),
        "VDOC1:0700000000000000010000000100000004000000612e6d64030000006f6c6400000000"
    );
    let typed = original
        .clone()
        .with_number("score", -0.0)
        .unwrap()
        .with_date("modified", day("2024-02-29"))
        .unwrap();
    assert_eq!(
        Document::from_payload(&typed.to_payload().unwrap()).unwrap(),
        typed
    );
    assert!(typed.to_payload().unwrap().starts_with("VDOC2:"));
    fn encode(bytes: &[u8]) -> String {
        format!(
            "VDOC2:{}",
            bytes.iter().map(|b| format!("{b:02x}")).collect::<String>()
        )
    }
    let base = original.to_payload().unwrap();
    let base = base.as_bytes()[6..]
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| u8::from_str_radix(std::str::from_utf8(p).unwrap(), 16).unwrap())
        .collect::<Vec<_>>();
    let numeric = |value: f64| {
        let mut bytes = base.clone();
        bytes.extend(1u32.to_le_bytes());
        bytes.extend(1u32.to_le_bytes());
        bytes.push(b'x');
        bytes.extend(value.to_le_bytes());
        bytes.extend(0u32.to_le_bytes());
        bytes
    };
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(Document::from_payload(&encode(&numeric(value))).is_err());
    }
    let bytes = numeric(1.0);
    for cut in 0..bytes.len() {
        assert!(Document::from_payload(&encode(&bytes[..cut])).is_err());
    }
    let mut bad_date = base.clone();
    bad_date.extend(0u32.to_le_bytes());
    bad_date.extend(1u32.to_le_bytes());
    bad_date.extend(1u32.to_le_bytes());
    bad_date.push(b'd');
    bad_date.extend(20260229u32.to_le_bytes());
    assert!(Document::from_payload(&encode(&bad_date)).is_err());
    let mut duplicate = numeric(1.0);
    duplicate.truncate(duplicate.len() - 4);
    duplicate[base.len()..base.len() + 4].copy_from_slice(&2u32.to_le_bytes());
    duplicate.extend(1u32.to_le_bytes());
    duplicate.push(b'x');
    duplicate.extend(2f64.to_le_bytes());
    duplicate.extend(0u32.to_le_bytes());
    assert!(Document::from_payload(&encode(&duplicate)).is_err());
    let mut excessive = base;
    excessive.extend(33u32.to_le_bytes());
    assert!(Document::from_payload(&encode(&excessive)).is_err());
    assert!(original.clone().with_number("", 1.0).is_err());
    assert!(original.with_date("x\0", day("2026-10-15")).is_err());
}

#[test]
fn cli_applies_where_to_all_modes_and_rejects_invalid_expressions() {
    let temp = TempDir::new();
    let mut c = Collection::create(temp.path(), "docs", Config::new(1), "fixture").unwrap();
    for document in documents() {
        c.put(&document, &[document.id as f32]).unwrap();
    }
    c.checkpoint().unwrap();
    drop(c);
    for (mode, query) in [
        ("docs-search", vec!["100", "10"]),
        ("docs-keywords", vec!["common", "10"]),
        ("docs-hybrid", vec!["100", "common", "10"]),
    ] {
        let mut command = Command::new(env!("CARGO_BIN_EXE_vecnook"));
        command
            .args([
                mode,
                temp.path().to_str().unwrap(),
                "docs",
                "1",
                "fixture",
                "l2",
            ])
            .args(query)
            .args([
                "--where",
                r#"number("score") == 0.25 AND NOT tag("bucket-0")"#,
            ]);
        let output = command.output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let text = String::from_utf8(output.stdout).unwrap();
        assert!(text.contains("\"id\":\"101\""), "{text}");
        assert!(text.contains("\"numbers\":{\"score\":0.25}"));
        assert!(text.contains("\"dates\":{\"modified\":"));
        let output = command.args(["--where", "bad"]).output().unwrap();
        assert!(!output.status.success());
    }
}

#[test]
fn worker_accepts_owned_compound_filters_and_rejects_invalid_bounds_before_enqueue() {
    let temp = TempDir::new();
    let mut c = Collection::create(temp.path(), "docs", Config::new(1), "fixture").unwrap();
    c.put(
        &Document::new(1, "common", "a.md")
            .with_number("score", 2.0)
            .unwrap(),
        &[1.0],
    )
    .unwrap();
    let (client, worker) = vecnook::app::start(c).unwrap();
    let found = client
        .submit_filtered(
            "fixture",
            vec![1.0],
            3,
            FilterExpr::parse(r#"number("score") >= 2"#).unwrap(),
        )
        .unwrap()
        .recv()
        .unwrap()
        .unwrap();
    assert_eq!(found.neighbors[0].document.id, 1);
    assert!(
        client
            .submit_filtered(
                "fixture",
                vec![1.0],
                3,
                FilterExpr::number_range("score", Included(num(3.0)), Included(num(2.0)))
            )
            .is_err()
    );
    drop(client);
    drop(worker.join().unwrap());
}

#[test]
fn invalid_programmatic_bounds_are_rejected_by_all_search_entry_points() {
    let temp = TempDir::new();
    let mut c = Collection::create(temp.path(), "docs", Config::new(1), "fixture").unwrap();
    let doc = Document::new(1, "common", "a.md")
        .with_number("score", 2.0)
        .unwrap();
    c.put(&doc, &[1.0]).unwrap();
    let expression = FilterExpr::number_range("score", Included(num(3.0)), Included(num(1.0)));
    let filter = DocumentFilter::default().with_expression(&expression);
    for k in [0, 1] {
        assert!(matches!(
            c.search(&[1.0], k, SearchOptions::default(), filter),
            Err(vecnook::Error::InvalidInput(_))
        ));
        assert!(matches!(
            c.search_keywords("common", k, filter),
            Err(vecnook::Error::InvalidInput(_))
        ));
        assert!(matches!(
            c.search_hybrid(&[1.0], "common", k, HybridSearchOptions::default(), filter),
            Err(vecnook::Error::InvalidInput(_))
        ));
    }
    assert_eq!(c.sequence(), 1);
    assert_eq!(c.get(1).unwrap(), Some(doc));
}

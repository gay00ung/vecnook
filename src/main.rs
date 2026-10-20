#![forbid(unsafe_code)]
use std::{
    fmt::Display,
    fs::File,
    io::{self, BufRead, Read, Write},
    process::{Command, ExitCode, Stdio},
    str::FromStr,
};

use vecnook::{
    Collection, Config, Database, Document, DocumentFilter, DocumentMutation, EmbeddingSpace,
    Error, FilterExpr, HybridSearchOptions, KeywordAnalyzer, MaintenancePolicy, Metric, Mutation,
    Result, SearchOptions, SearchStrategy,
    bench::{self, BenchConfig, Dataset},
};

mod demo;
mod demo_data;

const HELP: &str = "vecnook: dependency-free vector database\n\
Usage:\n\
  vecnook env-check [--model name] [--db collection-dir] [--json]\n\
  vecnook docs-export-jsonl --db collection-dir --output new-file\n\
  vecnook docs-import-jsonl input-file --db new-collection-dir\n\
  vecnook docs-reindex folder --db existing-dir --to new-dir --model name\n\
  vecnook demo [demo-root] [prepared-query=1..3]\n\
  vecnook docs-sync <markdown-folder> --db <collection-dir> [--model name] [--tag value] [--namespace name] [--prune] [--record-metadata|--no-record-metadata]\n\
  vecnook docs-find <question> --db <collection-dir> [--mode auto|hybrid|vector|keyword] [--k N] [--json] [--tag value] [--source value] [--where expression]\n\
  vecnook init <db-dir> <dimensions> [m] [ef-construction] [seed] [--metric l2|cosine|ip]\n\
  vecnook put <db-dir> <id> <comma-separated-vector> [metadata]\n\
  vecnook get <db-dir> <id>\n\
  vecnook delete <db-dir> <id>\n\
  vecnook search <db-dir> <comma-separated-vector> [k] [ef-search] [exact|hnsw|auto] [--metadata value] [--json]\n\
  vecnook batch <db-dir> <tsv-file>\n\
  vecnook backup <db-dir> <new-backup-dir>\n\
  vecnook maintain <db-dir>\n\
  vecnook doctor <db-or-collection-dir>\n\
  vecnook export <db-dir> <new-export-file>\n\
  vecnook import <export-file> <new-db-dir>\n\
  vecnook docs-import <export-file> <root> <new-name>\n\
  vecnook docs-export <root> <name> <dimensions> <model-identity> <metric> <new-export-file>\n\
  vecnook docs-backup <root> <name> <dimensions> <model-identity> <metric> <backup-root> <new-name>\n\
  vecnook stats <db-dir>\n\
  vecnook checkpoint <db-dir>\n\
  vecnook compact <db-dir>\n\
  vecnook shell <db-dir>\n\
  vecnook docs-init <root> <name> <dimensions> <model-identity> <l2|cosine|ip>\n\
  vecnook docs-inspect <root> <name>\n\
  vecnook docs-context <root> <name> <dimensions> <model-identity> <metric> <id> [radius] [--where expression]\n\
  vecnook docs-get <root> <name> <dimensions> <model-identity> <metric> <id> [--vector]\n\
  vecnook docs-compact <root> <name> <dimensions> <model-identity> <metric>\n\
  vecnook docs-info <root> <name>\n\
  vecnook docs-list <root> <name> <dimensions> <model-identity> <metric> [after-id|-] [limit=128]\n\
  vecnook docs-batch <root> <name> <dimensions> <model-identity> <metric> <tsv-file> [--if-sequence N]\n\
  vecnook docs-search <root> <name> <dimensions> <model-identity> <metric> <vector> [k] [ef] [auto|hnsw|exact] [--tag value] [--source value] [--where expression]\n\
  vecnook docs-keywords <root> <name> <dimensions> <model-identity> <metric> <text> [k] [--tag value] [--source value] [--where expression]\n\
  vecnook docs-hybrid <root> <name> <dimensions> <model-identity> <metric> <vector> <text> [k] [--candidates N] [--rrf-k N] [--tag value] [--source value] [--where expression]\n\
  vecnook docs-checkpoint <root> <name> <dimensions> <model-identity> <metric>\n\
  vecnook bench [count=10000] [dimensions=512] [queries=200] [ef=128] [seed=42] [clustered|uniform] [l2|cosine|ip]\n\
  vecnook bench-file <base.fvecs> <query.fvecs> [count=10000] [queries=200] [ef=128] [l2|cosine|ip]\n\n\
Global --json-errors before the command emits structured failures on stderr.\n\
Document commands also accept --db collection-dir instead of the identity prefix.\n\
Distances sort ascending. The shell accepts the same DB commands without <db-dir>, plus quit.\n";

fn main() -> ExitCode {
    let mut args: Vec<String> = std::env::args().skip(1).collect();
    let json_errors = args.first().is_some_and(|s| s == "--json-errors");
    if json_errors {
        args.remove(0);
    }
    match run(&args, json_errors) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            if json_errors {
                eprintln!(
                    "{{\"schema_version\":1,\"error\":{{\"kind\":{},\"message\":{}}}}}",
                    json_string(&format!("{:?}", error.kind())),
                    json_string(&error.to_string())
                );
            } else {
                eprintln!("ERROR {error}");
            }
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String], json_errors: bool) -> Result<()> {
    let Some(command) = args.first().map(String::as_str) else {
        print!("{HELP}");
        return Ok(());
    };
    if matches!(command, "--version" | "-V") {
        println!("vecnook {}", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    if matches!(command, "help" | "--help" | "-h") {
        print!("{HELP}");
        return Ok(());
    }
    if args.len() == 2
        && matches!(args[1].as_str(), "--help" | "-h")
        && !matches!(
            command,
            "docs-sync"
                | "docs-find"
                | "env-check"
                | "docs-export-jsonl"
                | "docs-import-jsonl"
                | "docs-reindex"
        )
    {
        let prefix = format!("vecnook {command} ");
        let usage: Vec<_> = HELP
            .lines()
            .filter(|line| line.trim_start().starts_with(&prefix))
            .collect();
        if usage.is_empty() {
            return Err(Error::InvalidInput(format!("unknown command {command}")));
        }
        println!("{}", usage.join("\n"));
        if command.starts_with("docs-") {
            println!(
                "Or replace root/name/identity with --db <collection-dir>.\nKeyword/hybrid queries support --analyzer words|korean-ngram.\ndocs-list/get support --vectors/--vector respectively."
            );
        }
        return Ok(());
    }
    if matches!(
        command,
        "docs-sync"
            | "docs-find"
            | "env-check"
            | "docs-export-jsonl"
            | "docs-import-jsonl"
            | "docs-reindex"
    ) {
        return markdown_command(args, json_errors);
    }
    if command.starts_with("docs-") && args.iter().any(|s| s == "--db") {
        return document_alias(args);
    }
    if command.starts_with("docs-") {
        return document_command(args);
    }
    if command == "demo" {
        return demo::run(&args[1..]);
    }
    if command == "doctor" {
        require_count(args, 2, 2, "doctor <db-or-collection-directory>")?;
        let report = vecnook::doctor(&args[1])?;
        println!("{}", diagnostic_json(&report));
        return Ok(());
    }
    if command == "import" {
        require_count(args, 3, 3, "import <export-file> <new-db-directory>")?;
        let db = Database::import(&args[1], &args[2])?;
        println!("OK imported records={}", db.len());
        return Ok(());
    }
    if command == "init" {
        require_count(
            args,
            3,
            8,
            "init <db-dir> <dimensions> [m] [ef-construction] [seed] [--metric l2|cosine|ip]",
        )?;
        let mut config = Config::new(parse(&args[2], "dimensions")?);
        let extras = &args[3..];
        let numeric = if let Some(position) = extras.iter().position(|arg| arg == "--metric") {
            if position + 2 != extras.len() {
                return Err(Error::InvalidInput(
                    "--metric requires one final value".into(),
                ));
            }
            config.metric = metric(&extras[position + 1])?;
            &extras[..position]
        } else {
            extras
        };
        if numeric.len() > 3 {
            return Err(Error::InvalidInput("too many init parameters".into()));
        }
        if let Some(value) = numeric.first() {
            config.m = parse(value, "M")?;
        }
        if let Some(value) = numeric.get(1) {
            config.ef_construction = parse(value, "efConstruction")?;
        }
        if let Some(value) = numeric.get(2) {
            config.seed = parse(value, "seed")?;
        }
        let db = Database::create(&args[1], config)?;
        println!(
            "OK initialized dimensions={} m={} ef_construction={} seed={} metric={}",
            db.config().dimensions,
            db.config().m,
            db.config().ef_construction,
            db.config().seed,
            db.config().metric.name()
        );
        return Ok(());
    }
    if command == "bench" {
        return benchmark(&args[1..]);
    }
    if command == "bench-file" {
        require_count(
            args,
            3,
            7,
            "bench-file <base> <query> [count] [queries] [ef] [metric]",
        )?;
        let result = bench::run_fvecs(
            &args[1],
            &args[2],
            optional(args, 3, 10_000, "count")?,
            optional(args, 4, 200, "queries")?,
            optional(args, 5, 128, "efSearch")?,
            metric(args.get(6).map_or("l2", String::as_str))?,
        )?;
        print_benchmark(result);
        return Ok(());
    }
    if args.len() < 2 {
        return Err(Error::InvalidInput(
            "missing database directory; see vecnook --help".into(),
        ));
    }
    if !matches!(
        command,
        "put"
            | "get"
            | "delete"
            | "search"
            | "stats"
            | "checkpoint"
            | "compact"
            | "shell"
            | "batch"
            | "backup"
            | "export"
            | "maintain"
    ) {
        return Err(Error::InvalidInput(format!("unknown command {command}")));
    }
    if command == "shell" {
        require_count(args, 2, 2, "shell <db-dir>")?;
    }
    let mut db = Database::open(&args[1])?;
    if db.recovery_info().truncated_tail_bytes != 0 {
        eprintln!(
            "RECOVERY truncated_tail_bytes={}",
            db.recovery_info().truncated_tail_bytes
        );
    }
    if command == "shell" {
        shell(&mut db)
    } else {
        println!("{}", execute(&mut db, command, &args[2..])?);
        Ok(())
    }
}

// Ship the existing standard-library Markdown client inside the installed binary,
// so folder sync does not require a repository checkout or a downloaded script.
fn markdown_command(args: &[String], json_errors: bool) -> Result<()> {
    let python = std::env::var_os("VECNOOK_PYTHON")
        .unwrap_or_else(|| if cfg!(windows) { "python" } else { "python3" }.into());
    let mut command = Command::new(python);
    command
        .env("PYTHONIOENCODING", "utf-8")
        .env("VECNOOK_COMMAND", &args[0])
        .arg("-c")
        .arg("import sys; exec(compile(sys.stdin.buffer.read(), '<vecnook-documents>', 'exec'))")
        .stdin(Stdio::piped())
        .arg("--binary")
        .arg(std::env::current_exe()?);
    match args[0].as_str() {
        "docs-sync" => {
            command.arg("sync");
        }
        "docs-find" => {
            command.args(["search", "--mode", "auto"]);
        }
        "env-check" => {
            command.arg("env-check");
        }
        "docs-export-jsonl" => {
            command.arg("export-jsonl");
        }
        "docs-import-jsonl" => {
            command.arg("import-jsonl");
        }
        "docs-reindex" => {
            command.arg("reindex");
        }
        _ => unreachable!(),
    }
    if json_errors {
        command.stdout(Stdio::piped()).stderr(Stdio::piped());
    } else {
        command.stdout(Stdio::inherit()).stderr(Stdio::inherit());
    }
    let mut child = command.args(&args[1..]).spawn().map_err(|error| {
        Error::InvalidInput(format!(
            "Markdown commands require Python 3.10+ (set VECNOOK_PYTHON to its path): {error}"
        ))
    })?;
    // Keep the embedded source out of argv: Windows limits command lines to
    // 32767 UTF-16 units. The wrapper consumes stdin before executing any client work.
    let sent = child
        .stdin
        .take()
        .ok_or_else(|| Error::InvalidInput("missing Python input pipe".into()))?
        .write_all(include_bytes!("../examples/documents/search.py"));
    let output = child.wait_with_output()?;
    if json_errors {
        io::stdout().write_all(&output.stdout)?;
    }
    if !output.status.success() {
        return Err(Error::InvalidInput(if json_errors {
            String::from_utf8_lossy(&output.stderr).trim().to_string()
        } else {
            "Markdown command failed; see the message above".into()
        }));
    }
    sent?;
    Ok(())
}

fn execute(db: &mut Database, command: &str, args: &[String]) -> Result<String> {
    match command {
        "put" => {
            require_count(args, 2, 3, "put <id> <vector> [metadata]")?;
            let id = parse(&args[0], "ID")?;
            let coordinates = vector_values(&args[1])?;
            let inserted = db.put(id, &coordinates, args.get(2).map_or("", String::as_str))?;
            Ok(format!(
                "OK {} id={id} sequence={}",
                if inserted { "inserted" } else { "updated" },
                db.sequence()
            ))
        }
        "get" => {
            require_count(args, 1, 1, "get <id>")?;
            let id = parse(&args[0], "ID")?;
            let record = db
                .get(id)
                .ok_or_else(|| Error::InvalidInput(format!("ID {id} not found")))?;
            Ok(format!(
                "id={} vector={:?} metadata={:?}",
                record.id, record.vector, record.metadata
            ))
        }
        "delete" => {
            require_count(args, 1, 1, "delete <id>")?;
            let id = parse(&args[0], "ID")?;
            let deleted = db.delete(id)?;
            Ok(format!(
                "OK {} id={id} sequence={}",
                if deleted { "deleted" } else { "absent" },
                db.sequence()
            ))
        }
        "search" => {
            let json = args.last().is_some_and(|value| value == "--json");
            let args = if json { &args[..args.len() - 1] } else { args };
            require_count(
                args,
                1,
                6,
                "search <vector> [k] [ef] [exact|hnsw|auto] [--metadata value]",
            )?;
            let query = cli_vector(&args[0])?;
            let k = optional(args, 1, 10, "K")?;
            let ef = optional(args, 2, 128, "efSearch")?;
            let mode = args.get(3).map_or("auto", String::as_str);
            let strategy = match mode {
                "exact" => SearchStrategy::Exact,
                "hnsw" => SearchStrategy::Hnsw,
                "auto" => SearchStrategy::Auto,
                _ => {
                    return Err(Error::InvalidInput(
                        "search mode must be exact, hnsw or auto".into(),
                    ));
                }
            };
            let filter = if args.len() > 4 {
                if args.len() != 6 || args[4] != "--metadata" {
                    return Err(Error::InvalidInput(
                        "expected --metadata <exact value>".into(),
                    ));
                }
                Some(args[5].as_str())
            } else {
                None
            };
            let options = SearchOptions::default()
                .with_strategy(strategy)
                .with_ef_search(ef);
            let found = match filter {
                Some(value) => db.search_metadata(&query, k, options, value)?,
                None => db.search(&query, k, options)?,
            };
            if json {
                return Ok(search_json(&found));
            }
            let mut output = format!(
                "mode={} requested={k} returned={} complete={} distance_computations={} filter_evaluations={} metric={} eligible={} reason={:?}",
                found.mode.name(),
                found.neighbors.len(),
                found.complete,
                found.distance_computations,
                found.filter_evaluations,
                found.metric.name(),
                found.eligible_count,
                found.reason
            );
            for neighbor in found.neighbors {
                output.push_str(&format!(
                    "\nid={} {}={:.9} metadata={:?}",
                    neighbor.id,
                    found.metric.name(),
                    neighbor.distance,
                    neighbor.metadata
                ));
            }
            Ok(output)
        }
        "stats" => {
            require_count(args, 0, 0, "stats")?;
            let stats = db.stats();
            let recovery = db.recovery_info();
            let maintenance = db.maintenance_status(MaintenancePolicy::default())?;
            Ok(format!(
                "dimensions={} m={} ef_construction={} seed={}\nactive={} physical={} tombstones={} layers={} directed_edges={} raw_vector_bytes={} sequence={}\nreplayed_frames={} skipped_frames={} truncated_tail_bytes={}\nmetric={} graph_cache_loaded={} cached_nodes={} graph_cache_note={:?}\nwal_bytes={} tombstone_ratio={:.6} checkpoint_recommended={} compact_recommended={}",
                db.config().dimensions,
                db.config().m,
                db.config().ef_construction,
                db.config().seed,
                stats.active_records,
                stats.physical_nodes,
                stats.tombstones,
                stats.layers,
                stats.directed_edges,
                stats.vector_bytes,
                db.sequence(),
                recovery.replayed_frames,
                recovery.skipped_frames,
                recovery.truncated_tail_bytes,
                db.config().metric.name(),
                recovery.graph_cache_loaded,
                recovery.cached_nodes,
                recovery.graph_cache_note,
                maintenance.wal_bytes,
                maintenance.tombstone_ratio,
                maintenance.checkpoint_recommended,
                maintenance.compact_recommended
            ))
        }
        "checkpoint" => {
            require_count(args, 0, 0, "checkpoint")?;
            db.checkpoint()?;
            Ok(format!("OK checkpoint sequence={}", db.sequence()))
        }
        "export" => {
            require_count(args, 1, 1, "export <new-export-file>")?;
            db.export(&args[0])?;
            Ok(format!("OK exported records={}", db.len()))
        }
        "backup" => {
            require_count(args, 1, 1, "backup <new-directory>")?;
            db.backup(&args[0])?;
            Ok(format!(
                "OK backup sequence={} path={:?}",
                db.sequence(),
                args[0]
            ))
        }
        "maintain" => {
            require_count(args, 0, 0, "maintain")?;
            let report = db.maintain(MaintenancePolicy::default())?;
            Ok(format!(
                "OK maintenance action={:?} removed_nodes={}",
                report.action, report.removed_nodes
            ))
        }
        "batch" => {
            require_count(args, 1, 1, "batch <tsv-file>")?;
            let rows = read_batch(&args[0])?;
            let operations: Vec<_> = rows
                .iter()
                .map(|r| match &r.vector {
                    Some(vector) => Mutation::Put {
                        id: r.id,
                        vector,
                        metadata: &r.metadata,
                    },
                    None => Mutation::Delete { id: r.id },
                })
                .collect();
            let report = db.write_batch(&operations)?;
            Ok(format!(
                "OK batch sequence={} inserted={} updated={} deleted={} absent={}",
                report.sequence, report.inserted, report.updated, report.deleted, report.absent
            ))
        }
        "compact" => {
            require_count(args, 0, 0, "compact")?;
            let removed = db.compact()?;
            Ok(format!(
                "OK compact removed_nodes={removed} active={}",
                db.stats().active_records
            ))
        }
        _ => Err(Error::InvalidInput(format!(
            "unknown shell command {command}"
        ))),
    }
}

fn json_string(value: &str) -> String {
    let mut result = String::from("\"");
    for c in value.chars() {
        match c {
            '"' => result.push_str("\\\""),
            '\\' => result.push_str("\\\\"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            c if c <= '\u{1f}' => result.push_str(&format!("\\u{:04x}", c as u32)),
            c => result.push(c),
        }
    }
    result.push('"');
    result
}

fn document_json(document: &Document) -> String {
    let mut result = format!(
        "{{\"schema_version\":1,\"id\":{},\"source\":{},\"text\":{},\"start_line\":{},\"end_line\":{},\"tags\":[{}]}}",
        json_string(&document.id.to_string()),
        json_string(&document.source),
        json_string(&document.text),
        document.start_line,
        document.end_line,
        document
            .tags
            .iter()
            .map(|tag| json_string(tag))
            .collect::<Vec<_>>()
            .join(",")
    );
    if !document.numbers.is_empty() || !document.dates.is_empty() {
        result.pop();
        if !document.numbers.is_empty() {
            let fields = document
                .numbers
                .iter()
                .map(|(key, value)| format!("{}:{}", json_string(key), value))
                .collect::<Vec<_>>()
                .join(",");
            result.push_str(&format!(",\"numbers\":{{{fields}}}"));
        }
        if !document.dates.is_empty() {
            let fields = document
                .dates
                .iter()
                .map(|(key, value)| {
                    format!("{}:{}", json_string(key), json_string(&value.to_string()))
                })
                .collect::<Vec<_>>()
                .join(",");
            result.push_str(&format!(",\"dates\":{{{fields}}}"));
        }
        result.push('}');
    }
    result
}

fn document_with_vector_json(collection: &Collection, doc: &Document, include: bool) -> String {
    let mut json = document_json(doc);
    if include {
        json.pop();
        let vector = collection
            .vector(doc.id)
            .unwrap_or(&[])
            .iter()
            .map(|v| format!("{v:?}"))
            .collect::<Vec<_>>()
            .join(",");
        json.push_str(&format!(",\"vector\":[{vector}]}}"));
    }
    json
}

fn document_alias(args: &[String]) -> Result<()> {
    let mut rest = args[1..].to_vec();
    let index = rest.iter().position(|s| s == "--db").unwrap();
    if index + 1 >= rest.len() {
        return Err(Error::InvalidInput("--db needs a collection path".into()));
    }
    let path = std::path::PathBuf::from(rest.remove(index + 1));
    rest.remove(index);
    let name = path
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or_else(|| Error::InvalidInput("invalid collection path".into()))?;
    let root = path.parent().unwrap_or_else(|| std::path::Path::new("."));
    let root = if root.as_os_str().is_empty() {
        std::path::Path::new(".")
    } else {
        root
    };
    let space = Collection::describe(root, name)?;
    let mut expanded = vec![
        args[0].clone(),
        root.to_string_lossy().into_owned(),
        name.to_string(),
    ];
    if !matches!(args[0].as_str(), "docs-info" | "docs-inspect") {
        expanded.extend([
            space.dimensions.to_string(),
            space.model,
            match space.metric {
                Metric::SquaredL2 => "l2",
                Metric::Cosine => "cosine",
                Metric::InnerProduct => "ip",
                _ => return Err(Error::InvalidInput("unsupported metric".into())),
            }
            .to_string(),
        ]);
    }
    expanded.extend(rest);
    document_command(&expanded)
}

fn document_command(args: &[String]) -> Result<()> {
    let command = args[0].as_str();
    if command == "docs-import" {
        require_count(args, 4, 4, "docs-import <export-file> <root> <new-name>")?;
        let collection = Collection::import(&args[1], &args[2], &args[3])?;
        println!("OK imported documents={}", collection.len());
        return Ok(());
    }
    if command == "docs-inspect" {
        require_count(args, 3, 3, "docs-inspect <root> <name>")?;
        let path = std::path::Path::new(&args[1]).join(&args[2]);
        let inspection = vecnook::inspect_documents(path)?;
        let report = inspection.diagnostics;
        let mut rows = inspection.documents;
        rows.sort_unstable_by_key(|(d, _)| d.id);
        let documents = rows
            .iter()
            .map(|(d, v)| {
                let mut json = document_json(d);
                json.pop();
                let coordinates = v
                    .iter()
                    .map(|v| format!("{v:?}"))
                    .collect::<Vec<_>>()
                    .join(",");
                json.push_str(&format!(",\"vector\":[{coordinates}]}}"));
                json
            })
            .collect::<Vec<_>>()
            .join(",");
        println!(
            "{{\"schema_version\":1,\"sequence\":\"{}\",\"diagnostics\":{},\"documents\":[{}]}}",
            report.sequence,
            diagnostic_json(&report),
            documents
        );
        return Ok(());
    }
    if command == "docs-info" {
        require_count(args, 3, 3, "docs-info <root> <name>")?;
        let space = Collection::describe(&args[1], &args[2])?;
        println!(
            "{{\"schema_version\":1,\"name\":{},\"model\":{},\"dimensions\":{},\"metric\":{}}}",
            json_string(&args[2]),
            json_string(&space.model),
            space.dimensions,
            json_string(space.metric.name())
        );
        return Ok(());
    }
    require_count(
        args,
        6,
        90,
        "docs-<command> <root> <name> <dimensions> <model-identity> <metric> [parameters]",
    )?;
    let space = EmbeddingSpace::new(&args[4], parse(&args[3], "dimensions")?, metric(&args[5])?);
    if command == "docs-init" {
        require_count(
            args,
            6,
            6,
            "docs-init <root> <name> <dimensions> <model-identity> <metric>",
        )?;
        Collection::create(
            &args[1],
            &args[2],
            Config::new(space.dimensions).with_metric(space.metric),
            &space.model,
        )?;
        println!("OK initialized collection={}", args[2]);
        return Ok(());
    }
    if !matches!(
        command,
        "docs-batch"
            | "docs-list"
            | "docs-export"
            | "docs-backup"
            | "docs-search"
            | "docs-keywords"
            | "docs-hybrid"
            | "docs-checkpoint"
            | "docs-get"
            | "docs-context"
            | "docs-compact"
    ) {
        return Err(Error::InvalidInput("unknown document command".into()));
    }
    let mut collection = Collection::open(&args[1], &args[2], &space)?;
    let mut extra_owned = args[6..].to_vec();
    if let Some(i) = extra_owned.iter().position(|s| s == "--analyzer") {
        if !matches!(command, "docs-keywords" | "docs-hybrid") || i + 1 >= extra_owned.len() {
            return Err(Error::InvalidInput(
                "--analyzer requires words|korean-ngram on keyword/hybrid queries".into(),
            ));
        }
        let analyzer = match extra_owned[i + 1].as_str() {
            "words" => KeywordAnalyzer::Words,
            "korean-ngram" => KeywordAnalyzer::KoreanNgram,
            _ => return Err(Error::InvalidInput("unknown keyword analyzer".into())),
        };
        collection.set_keyword_analyzer(analyzer)?;
        extra_owned.drain(i..i + 2);
    }
    let extra = &extra_owned[..];
    match command {
        "docs-keywords" | "docs-hybrid" => {
            let hybrid = command == "docs-hybrid";
            let prefix = if hybrid { 2 } else { 1 };
            require_count(
                extra,
                prefix,
                80,
                "docs-keywords ... <text> [k]; docs-hybrid ... <vector> <text> [k] [flags]",
            )?;
            let flag = extra[prefix..]
                .iter()
                .position(|s| s.starts_with("--"))
                .map_or(extra.len(), |i| prefix + i);
            let positional = &extra[..flag];
            require_count(
                positional,
                prefix,
                prefix + 1,
                "unexpected keyword/hybrid positional arguments",
            )?;
            let k = optional(positional, prefix, 10usize, "K")?;
            let flags = &extra[flag..];
            if !flags.len().is_multiple_of(2) {
                return Err(Error::InvalidInput(
                    "document search flags need values".into(),
                ));
            }
            let mut source = None;
            let mut tags = Vec::new();
            let mut expression = None;
            let mut candidates = None;
            let mut rrf_k = None;
            for pair in flags.as_chunks::<2>().0 {
                match pair[0].as_str() {
                    "--source" if source.is_none() => source = Some(pair[1].as_str()),
                    "--tag" => tags.push(pair[1].as_str()),
                    "--where" if expression.is_none() => {
                        expression = Some(FilterExpr::parse(&pair[1])?)
                    }
                    "--candidates" if hybrid && candidates.is_none() => {
                        candidates = Some(parse(&pair[1], "candidates")?)
                    }
                    "--rrf-k" if hybrid && rrf_k.is_none() => {
                        rrf_k = Some(parse(&pair[1], "RRF offset")?)
                    }
                    _ => {
                        return Err(Error::InvalidInput(
                            "unknown/duplicate document search flag".into(),
                        ));
                    }
                }
            }
            let filter = DocumentFilter::default()
                .with_optional_source(source)
                .with_tags(&tags)
                .with_optional_expression(expression.as_ref());
            if hybrid {
                let query = cli_vector(&extra[0])?;
                let options = HybridSearchOptions::default()
                    .with_candidates(candidates.unwrap_or(64))
                    .with_rrf_k(rrf_k.unwrap_or(60));
                let report = collection.search_hybrid(&query, &extra[1], k, options, filter)?;
                let matches = report.neighbors.iter().map(|n| format!(
                    "{{\"score\":{},\"vector_rank\":{},\"keyword_rank\":{},\"distance\":{},\"keyword_score\":{},\"document\":{}}}",
                    n.score, json_optional(n.vector_rank), json_optional(n.keyword_rank),
                    json_optional(n.distance), json_optional(n.keyword_score), document_json(&n.document)
                )).collect::<Vec<_>>().join(",");
                println!(
                    "{{\"schema_version\":1,\"mode\":\"hybrid\",\"complete\":{},\"candidate_count\":{},\"rrf_k\":{},\"search\":{},\"keyword\":{},\"matches\":[{}]}}",
                    report.complete,
                    report.candidate_count,
                    report.rrf_k,
                    search_json(&report.vector),
                    keyword_json(&report.keyword),
                    matches
                );
            } else {
                let report = collection.search_keywords(&extra[0], k, filter)?;
                let matches = report
                    .neighbors
                    .iter()
                    .map(|n| {
                        format!(
                            "{{\"score\":{},\"document\":{}}}",
                            n.score,
                            document_json(&n.document)
                        )
                    })
                    .collect::<Vec<_>>()
                    .join(",");
                println!(
                    "{{\"schema_version\":1,\"mode\":\"keyword\",\"keyword\":{},\"matches\":[{}]}}",
                    keyword_json(&report),
                    matches
                );
            }
        }
        "docs-export" => {
            require_count(extra, 1, 1, "docs-export ... <new-export-file>")?;
            collection.export(&extra[0])?;
            println!("OK exported documents={}", collection.len());
        }
        "docs-backup" => {
            require_count(extra, 2, 2, "docs-backup ... <root> <new-name>")?;
            collection.backup(&extra[0], &extra[1])?;
            println!("OK backup documents={}", collection.len());
        }
        "docs-batch" => {
            require_count(extra, 1, 3, "docs-batch ... <tsv-file> [--if-sequence N]")?;
            let expected = match extra {
                [_] => None,
                [_, flag, value] if flag == "--if-sequence" => {
                    Some(parse::<u64>(value, "sequence")?)
                }
                _ => return Err(Error::InvalidInput("expected --if-sequence N".into())),
            };
            let rows = read_batch(&extra[0])?;
            let documents = rows
                .iter()
                .map(|r| {
                    if r.vector.is_some() {
                        let doc = Document::from_payload(&r.metadata)?;
                        if doc.id != r.id {
                            return Err(Error::InvalidInput(
                                "document ID differs from batch ID".into(),
                            ));
                        }
                        Ok(Some(doc))
                    } else {
                        Ok(None)
                    }
                })
                .collect::<Result<Vec<_>>>()?;
            let operations: Vec<_> = rows
                .iter()
                .zip(&documents)
                .map(|(row, doc)| match doc {
                    Some(doc) => DocumentMutation::Put {
                        document: doc,
                        vector: row.vector.as_ref().unwrap(),
                    },
                    None => DocumentMutation::Delete { id: row.id },
                })
                .collect();
            let report = match expected {
                Some(sequence) => collection.write_batch_if_sequence(sequence, &operations)?,
                None => collection.write_batch(&operations)?,
            };
            println!(
                "OK documents inserted={} updated={} deleted={} sequence={}",
                report.inserted, report.updated, report.deleted, report.sequence
            );
        }
        "docs-list" => {
            let include_vectors = extra.last().is_some_and(|s| s == "--vectors");
            let extra = if include_vectors {
                &extra[..extra.len() - 1]
            } else {
                extra
            };
            require_count(
                extra,
                0,
                2,
                "docs-list ... [after-id|-] [limit=128] [--vectors]",
            )?;
            let after = extra
                .first()
                .filter(|s| s.as_str() != "-")
                .map(|s| parse::<u64>(s, "after ID"))
                .transpose()?;
            let limit = optional(extra, 1, 128usize, "limit")?;
            if !(1..=256).contains(&limit) {
                return Err(Error::InvalidInput("list limit must be 1..256".into()));
            }
            let page = collection
                .documents()
                .filter(|d| d.as_ref().map_or(true, |d| after.is_none_or(|a| d.id > a)))
                .take(limit + 1)
                .collect::<Result<Vec<_>>>()?;
            let next = if page.len() > limit {
                json_string(&page[limit - 1].id.to_string())
            } else {
                "null".into()
            };
            let documents = page
                .iter()
                .take(limit)
                .map(|doc| document_with_vector_json(&collection, doc, include_vectors))
                .collect::<Vec<_>>()
                .join(",");
            println!(
                "{{\"schema_version\":1,\"sequence\":\"{}\",\"next_after\":{},\"documents\":[{}]}}",
                collection.sequence(),
                next,
                documents
            );
        }
        "docs-search" => {
            require_count(
                extra,
                1,
                74,
                "docs-search ... <vector> [k] [ef] [strategy] [--tag value] [--source value] [--where expression]",
            )?;
            let query = cli_vector(&extra[0])?;
            let flag = extra
                .iter()
                .position(|s| s.starts_with("--"))
                .unwrap_or(extra.len());
            let positional = &extra[..flag];
            require_count(
                positional,
                1,
                4,
                "docs-search ... <vector> [k] [ef] [strategy]",
            )?;
            let k = optional(positional, 1, 10, "K")?;
            let ef_search = optional(positional, 2, 128, "efSearch")?;
            let strategy = match positional.get(3).map_or("auto", String::as_str) {
                "auto" => SearchStrategy::Auto,
                "exact" => SearchStrategy::Exact,
                "hnsw" => SearchStrategy::Hnsw,
                _ => {
                    return Err(Error::InvalidInput(
                        "invalid document search strategy".into(),
                    ));
                }
            };
            let flags = &extra[flag..];
            if !flags.len().is_multiple_of(2) {
                return Err(Error::InvalidInput(
                    "document filter flags need values".into(),
                ));
            }
            let mut source = None;
            let mut tags = Vec::new();
            let mut expression = None;
            for pair in flags.as_chunks::<2>().0 {
                match pair[0].as_str() {
                    "--source" if source.is_none() => source = Some(pair[1].as_str()),
                    "--tag" => tags.push(pair[1].as_str()),
                    "--where" if expression.is_none() => {
                        expression = Some(FilterExpr::parse(&pair[1])?)
                    }
                    _ => {
                        return Err(Error::InvalidInput(
                            "unknown/duplicate document filter flag".into(),
                        ));
                    }
                }
            }
            let report = collection.search(
                &query,
                k,
                SearchOptions::default()
                    .with_strategy(strategy)
                    .with_ef_search(ef_search),
                DocumentFilter::default()
                    .with_optional_source(source)
                    .with_tags(&tags)
                    .with_optional_expression(expression.as_ref()),
            )?;
            let documents = report
                .neighbors
                .iter()
                .map(|n| {
                    format!(
                        "{{\"distance\":{},\"document\":{}}}",
                        n.distance,
                        document_json(&n.document)
                    )
                })
                .collect::<Vec<_>>()
                .join(",");
            println!(
                "{{\"schema_version\":1,\"search\":{},\"matches\":[{}]}}",
                search_json(&report.search),
                documents
            );
        }
        "docs-context" => {
            require_count(
                extra,
                1,
                72,
                "docs-context ... <id> [radius=1] [--where expression] [--source value] [--tag value]",
            )?;
            let flag = extra
                .iter()
                .position(|s| s.starts_with("--"))
                .unwrap_or(extra.len());
            require_count(&extra[..flag], 1, 2, "docs-context ... <id> [radius]")?;
            let flags = &extra[flag..];
            if !flags.len().is_multiple_of(2) {
                return Err(Error::InvalidInput("context flags need values".into()));
            }
            let mut source = None;
            let mut tags = Vec::new();
            let mut expression = None;
            for pair in flags.as_chunks::<2>().0 {
                match pair[0].as_str() {
                    "--source" if source.is_none() => source = Some(pair[1].as_str()),
                    "--tag" => tags.push(pair[1].as_str()),
                    "--where" if expression.is_none() => {
                        expression = Some(FilterExpr::parse(&pair[1])?)
                    }
                    _ => return Err(Error::InvalidInput("unknown/duplicate context flag".into())),
                }
            }
            let documents = collection.context(
                parse(&extra[0], "ID")?,
                optional(&extra[..flag], 1, 1usize, "radius")?,
                DocumentFilter::default()
                    .with_optional_source(source)
                    .with_tags(&tags)
                    .with_optional_expression(expression.as_ref()),
            )?;
            println!(
                "{{\"schema_version\":1,\"documents\":[{}]}}",
                documents
                    .iter()
                    .map(document_json)
                    .collect::<Vec<_>>()
                    .join(",")
            );
        }
        "docs-get" => {
            require_count(extra, 1, 2, "docs-get ... <id> [--vector]")?;
            if extra.len() == 2 && extra[1] != "--vector" {
                return Err(Error::InvalidInput("expected --vector".into()));
            }
            let id = parse(&extra[0], "ID")?;
            let doc = collection
                .get(id)?
                .ok_or_else(|| Error::InvalidInput("document ID not found".into()))?;
            println!(
                "{}",
                document_with_vector_json(&collection, &doc, extra.len() == 2)
            );
        }
        "docs-checkpoint" => {
            require_count(extra, 0, 0, "docs-checkpoint ...")?;
            collection.checkpoint()?;
            println!("OK checkpoint");
        }
        "docs-compact" => {
            require_count(extra, 0, 0, "docs-compact ...")?;
            println!("OK removed={}", collection.compact()?);
        }
        _ => unreachable!(),
    }
    Ok(())
}

fn diagnostic_json(r: &vecnook::DiagnosticReport) -> String {
    let c = &r.capacity;
    format!(
        "{{\"schema_version\":1,\"sequence\":\"{}\",\"model\":{},\"dimensions\":{},\"metric\":{},\"active_records\":{},\"physical_nodes\":{},\"tombstones\":{},\"raw_vector_bytes\":{},\"estimated_snapshot_bytes\":{},\"remaining_snapshot_bytes\":{},\"remaining_nodes\":{},\"snapshot_file_bytes\":{},\"wal_bytes\":{},\"cache_bytes\":{},\"pending_tail_bytes\":{},\"replayed_frames\":{},\"graph_cache_valid\":{},\"graph_cache_note\":{},\"checkpoint_recommended\":{},\"compact_recommended\":{}}}",
        r.sequence,
        r.space
            .as_ref()
            .map_or("null".into(), |s| json_string(&s.model)),
        r.config.dimensions,
        json_string(r.config.metric.name()),
        c.active_records,
        c.physical_nodes,
        c.tombstones,
        c.vector_bytes,
        c.snapshot_bytes,
        c.remaining_snapshot_bytes,
        c.remaining_nodes,
        r.snapshot_file_bytes,
        r.wal_bytes,
        r.cache_bytes,
        r.pending_tail_bytes,
        r.replayed_frames,
        r.graph_cache_valid,
        r.graph_cache_note
            .as_ref()
            .map_or("null".into(), |s| json_string(s)),
        r.checkpoint_recommended,
        r.compact_recommended
    )
}

fn search_json(report: &vecnook::SearchReport) -> String {
    let neighbors: Vec<_> = report
        .neighbors
        .iter()
        .map(|n| {
            format!(
                "{{\"schema_version\":1,\"id\":{},\"distance\":{},\"metadata\":{}}}",
                json_string(&n.id.to_string()),
                n.distance,
                json_string(&n.metadata)
            )
        })
        .collect();
    format!(
        "{{\"schema_version\":1,\"mode\":\"{}\",\"metric\":\"{}\",\"complete\":{},\"eligible_count\":{},\"distance_computations\":{},\"filter_evaluations\":{},\"reason\":\"{:?}\",\"neighbors\":[{}]}}",
        report.mode.name(),
        report.metric.name(),
        report.complete,
        report.eligible_count,
        report.distance_computations,
        report.filter_evaluations,
        report.reason,
        neighbors.join(",")
    )
}

fn json_optional(value: Option<impl Display>) -> String {
    value.map_or_else(|| "null".to_owned(), |v| v.to_string())
}

fn keyword_json(report: &vecnook::KeywordSearchReport) -> String {
    format!(
        "{{\"eligible_count\":{},\"matching_count\":{},\"postings_visited\":{}}}",
        report.eligible_count, report.matching_count, report.postings_visited
    )
}

fn take_word(text: &str) -> (&str, &str) {
    let text = text.trim_start();
    let end = text.find(char::is_whitespace).unwrap_or(text.len());
    (&text[..end], text[end..].trim_start())
}

fn shell(db: &mut Database) -> Result<()> {
    let stdin = io::stdin();
    let mut stdout = io::stdout().lock();
    writeln!(stdout, "READY dimensions={}", db.config().dimensions)?;
    stdout.flush()?;
    for line in stdin.lock().lines() {
        let line = line?;
        let (command, rest) = take_word(&line);
        if command.is_empty() {
            continue;
        }
        if command == "quit" {
            writeln!(stdout, "BYE")?;
            stdout.flush()?;
            break;
        }
        let args: Vec<String> = if command == "put" {
            let (id, rest) = take_word(rest);
            let (coordinates, metadata) = take_word(rest);
            let mut args = vec![id.to_owned(), coordinates.to_owned()];
            if !metadata.is_empty() {
                args.push(metadata.to_owned());
            }
            args
        } else {
            rest.split_whitespace().map(str::to_owned).collect()
        };
        match execute(db, command, &args) {
            Ok(output) => writeln!(stdout, "{output}")?,
            Err(error) => writeln!(stdout, "ERR {error}")?,
        }
        stdout.flush()?;
    }
    Ok(())
}

fn parse<T: FromStr>(text: &str, name: &str) -> Result<T>
where
    T::Err: Display,
{
    text.parse()
        .map_err(|e| Error::InvalidInput(format!("invalid {name} {text:?}: {e}")))
}
fn optional<T: FromStr + Copy>(args: &[String], offset: usize, default: T, name: &str) -> Result<T>
where
    T::Err: Display,
{
    args.get(offset)
        .map_or(Ok(default), |text| parse(text, name))
}
fn cli_vector(text: &str) -> Result<Vec<f32>> {
    if let Some(path) = text.strip_prefix('@') {
        let mut body = String::new();
        File::open(path)?
            .take(128 * 1024 + 1)
            .read_to_string(&mut body)?;
        if body.len() > 128 * 1024 {
            return Err(Error::InvalidInput(
                "query vector file exceeds 128 KiB".into(),
            ));
        }
        return vector_values(body.trim());
    }
    vector_values(text)
}

fn vector_values(text: &str) -> Result<Vec<f32>> {
    let mut vector = Vec::new();
    for value in text.split(',') {
        if vector.len() == 4096 {
            return Err(Error::InvalidInput("too many vector coordinates".into()));
        }
        vector.push(parse(value.trim(), "coordinate")?);
    }
    Ok(vector)
}

fn metric(value: &str) -> Result<Metric> {
    match value {
        "l2" => Ok(Metric::SquaredL2),
        "cosine" => Ok(Metric::Cosine),
        "ip" => Ok(Metric::InnerProduct),
        _ => Err(Error::InvalidInput(
            "metric must be l2, cosine or ip".into(),
        )),
    }
}

struct BatchRow {
    id: u64,
    vector: Option<Vec<f32>>,
    metadata: String,
}
fn read_batch(path: &str) -> Result<Vec<BatchRow>> {
    let file = File::open(path)?;
    const LIMIT: u64 = 16 * 1024 * 1024;
    if file.metadata()?.len() > LIMIT {
        return Err(Error::InvalidInput("batch input exceeds 16 MiB".into()));
    }
    let mut text = String::new();
    file.take(LIMIT + 1).read_to_string(&mut text)?;
    if text.len() as u64 > LIMIT {
        return Err(Error::InvalidInput("batch input exceeds 16 MiB".into()));
    }
    let mut rows = Vec::new();
    for line in text.lines().filter(|line| !line.is_empty()) {
        if rows.len() == 1024 {
            return Err(Error::InvalidInput("batch exceeds 1024 operations".into()));
        }
        let fields: Vec<_> = line.splitn(4, '\t').collect();
        let row = match fields.as_slice() {
            ["put", id, vector] => BatchRow {
                id: parse(id, "ID")?,
                vector: Some(vector_values(vector)?),
                metadata: String::new(),
            },
            ["put", id, vector, metadata] => BatchRow {
                id: parse(id, "ID")?,
                vector: Some(vector_values(vector)?),
                metadata: (*metadata).to_owned(),
            },
            ["delete", id] => BatchRow {
                id: parse(id, "ID")?,
                vector: None,
                metadata: String::new(),
            },
            _ => {
                return Err(Error::InvalidInput(
                    "batch lines must be tab-separated put/ID/vector/metadata or delete/ID".into(),
                ));
            }
        };
        rows.push(row);
    }
    Ok(rows)
}
fn require_count(args: &[String], min: usize, max: usize, usage: &str) -> Result<()> {
    if (min..=max).contains(&args.len()) {
        Ok(())
    } else {
        Err(Error::InvalidInput(format!("usage: {usage}")))
    }
}

fn benchmark(args: &[String]) -> Result<()> {
    require_count(
        args,
        0,
        7,
        "bench [count] [dimensions] [queries] [ef] [seed] [clustered|uniform] [metric]",
    )?;
    let defaults = BenchConfig::default();
    let config = BenchConfig {
        count: optional(args, 0, defaults.count, "count")?,
        dimensions: optional(args, 1, defaults.dimensions, "dimensions")?,
        queries: optional(args, 2, defaults.queries, "queries")?,
        ef_search: optional(args, 3, defaults.ef_search, "efSearch")?,
        seed: optional(args, 4, defaults.seed, "seed")?,
        dataset: match args.get(5).map_or("clustered", String::as_str) {
            "clustered" => Dataset::Clustered,
            "uniform" => Dataset::Uniform,
            _ => {
                return Err(Error::InvalidInput(
                    "dataset must be clustered or uniform".into(),
                ));
            }
        },
        metric: metric(args.get(6).map_or("l2", String::as_str))?,
    };
    let result = bench::run(config)?;
    print_benchmark(result);
    Ok(())
}

fn print_benchmark(result: bench::BenchReport) {
    println!(
        "dataset={} count={} dimensions={} queries={} k={} ef_search={} seed={} metric={}",
        result.config.dataset.name(),
        result.config.count,
        result.config.dimensions,
        result.config.queries,
        result.k,
        result.config.ef_search,
        result.config.seed,
        result.config.metric.name()
    );
    println!(
        "build_seconds={:.6} recall_at_{}={:.4}% incomplete_queries={}",
        result.build_seconds,
        result.k,
        result.recall_at_k * 100.0,
        result.incomplete_queries
    );
    println!(
        "exact sequential_qps={:.2} p50_ms={:.6} p95_ms={:.6} p99_ms={:.6} mean_distance_computations={:.2}",
        result.exact.sequential_qps,
        result.exact.p50_ms,
        result.exact.p95_ms,
        result.exact.p99_ms,
        result.mean_exact_computations
    );
    println!(
        "hnsw sequential_qps={:.2} p50_ms={:.6} p95_ms={:.6} p99_ms={:.6} mean_distance_computations={:.2}",
        result.hnsw.sequential_qps,
        result.hnsw.p50_ms,
        result.hnsw.p95_ms,
        result.hnsw.p99_ms,
        result.mean_hnsw_computations
    );
    println!(
        "physical_nodes={} layers={} directed_edges={} raw_vector_bytes={}",
        result.index_stats.physical_nodes,
        result.index_stats.layers,
        result.index_stats.directed_edges,
        result.index_stats.vector_bytes
    );
}

#[cfg(test)]
mod json_tests {
    #[test]
    fn json_preserves_unicode_and_escapes_every_control_character() {
        assert_eq!(
            super::json_string("한글\"\\\n\r\t\0\u{1f}"),
            "\"한글\\\"\\\\\\n\\r\\t\\u0000\\u001f\""
        );
    }
}

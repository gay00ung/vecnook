#![forbid(unsafe_code)]
use std::{
    fmt::Display,
    fs::File,
    io::{self, BufRead, Read, Write},
    process::ExitCode,
    str::FromStr,
};

use vecnook::{
    Config, Database, Error, MaintenancePolicy, Metric, Mutation, Result, SearchMode,
    SearchOptions, SearchStrategy,
    bench::{self, BenchConfig, Dataset},
};

const HELP: &str = "vecnook: dependency-free vector database\n\
Usage:\n\
  vecnook init <db-dir> <dimensions> [m] [ef-construction] [seed] [--metric l2|cosine|ip]\n\
  vecnook put <db-dir> <id> <comma-separated-vector> [metadata]\n\
  vecnook get <db-dir> <id>\n\
  vecnook delete <db-dir> <id>\n\
  vecnook search <db-dir> <comma-separated-vector> [k] [ef-search] [exact|hnsw|auto] [--metadata value] [--json]\n\
  vecnook batch <db-dir> <tsv-file>\n\
  vecnook backup <db-dir> <new-backup-dir>\n\
  vecnook maintain <db-dir>\n\
  vecnook stats <db-dir>\n\
  vecnook checkpoint <db-dir>\n\
  vecnook compact <db-dir>\n\
  vecnook shell <db-dir>\n\
  vecnook bench [count=10000] [dimensions=512] [queries=200] [ef=128] [seed=42] [clustered|uniform] [l2|cosine|ip]\n\
  vecnook bench-file <base.fvecs> <query.fvecs> [count=10000] [queries=200] [ef=128] [l2|cosine|ip]\n\n\
Distances sort ascending. The shell accepts the same DB commands without <db-dir>, plus quit.\n";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("ERROR {error}");
            ExitCode::FAILURE
        }
    }
}

fn run(args: &[String]) -> Result<()> {
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
            let query = vector_values(&args[0])?;
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
            let found = db.search_filtered(
                &query,
                k,
                SearchOptions {
                    strategy,
                    ef_search: ef,
                    ..SearchOptions::default()
                },
                |r| filter.is_none_or(|value| r.metadata == value),
            )?;
            if json {
                return Ok(search_json(&found));
            }
            let mut output = format!(
                "mode={} requested={k} returned={} complete={} distance_computations={} metric={} eligible={} reason={:?}",
                match found.mode {
                    SearchMode::Exact => "exact",
                    SearchMode::Hnsw => "hnsw",
                },
                found.neighbors.len(),
                found.complete,
                found.distance_computations,
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

fn search_json(report: &vecnook::SearchReport) -> String {
    let neighbors: Vec<_> = report
        .neighbors
        .iter()
        .map(|n| {
            format!(
                "{{\"id\":{},\"distance\":{},\"metadata\":{}}}",
                n.id,
                n.distance,
                json_string(&n.metadata)
            )
        })
        .collect();
    format!(
        "{{\"mode\":\"{}\",\"metric\":\"{}\",\"complete\":{},\"eligible_count\":{},\"distance_computations\":{},\"reason\":\"{:?}\",\"neighbors\":[{}]}}",
        match report.mode {
            SearchMode::Exact => "exact",
            SearchMode::Hnsw => "hnsw",
        },
        report.metric.name(),
        report.complete,
        report.eligible_count,
        report.distance_computations,
        report.reason,
        neighbors.join(",")
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
        "exact sequential_qps={:.2} p50_ms={:.6} p95_ms={:.6} mean_distance_computations={:.2}",
        result.exact.sequential_qps,
        result.exact.p50_ms,
        result.exact.p95_ms,
        result.mean_exact_computations
    );
    println!(
        "hnsw sequential_qps={:.2} p50_ms={:.6} p95_ms={:.6} mean_distance_computations={:.2}",
        result.hnsw.sequential_qps,
        result.hnsw.p50_ms,
        result.hnsw.p95_ms,
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

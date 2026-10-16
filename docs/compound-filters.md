# Boolean and range filters

`Collection::search`, `search_keywords` and `search_hybrid` accept the same
`DocumentFilter`. Its existing source equality and all-tag conditions still
intersect. `with_expression` adds an indexed expression to that intersection.

```rust
use vecnook::{Date, Document, DocumentFilter, FilterExpr};

let document = Document::new(7, "Restore the local backup", "recovery.md")
    .with_number("priority", 0.8)?
    .with_date("modified", Date::parse("2026-10-15")?)?;
let expression = FilterExpr::parse(
    r#"(tag("rust") OR source("recovery.md")) AND NOT tag("archived")
       AND number("priority") >= 0.5 AND date("modified") >= "2026-10-01""#,
)?;
let filter = DocumentFilter::default().with_expression(&expression);
# let _ = (document, filter);
# Ok::<(), vecnook::Error>(())
```

Insert the document with `Collection::put` and pass `filter` to any search mode.
The app worker also accepts an owned expression through
`SearchClient::submit_filtered(model, vector, k, expression)`.

## Values and semantics

`Document.numbers` maps field names to finite `Number` values. `Number::new`
rejects NaN/infinity and normalizes negative zero. These are f64 quantities;
IDs retain their separate full-u64 identity and should not be stored as numeric
attributes when exact large integers matter. `Document.dates` maps field names
to validated Gregorian `Date` values, years 0001..9999, at calendar-day precision.
There is no timestamp/timezone conversion in the library.

Each type permits at most 32 fields. Keys contain 1..128 UTF-8 bytes without NUL.
The complete hex-encoded document must still fit the existing 16 KiB limit.
Numbers and dates have separate namespaces; one field name may occur in both.

`FilterExpr::all`, `any` and `negated` construct AND, OR and NOT. Empty AND matches
all active documents; empty OR matches none. NOT complements active documents,
excluding tombstones. A positive numeric/date range fails when the field is
missing. Consequently `NOT number("score") = 1` also includes missing scores.
Source, tag and field names compare exact UTF-8 strings with no normalization.

For programmatic intervals, `number_range` and `date_range` accept
`std::ops::Bound::{Included, Excluded, Unbounded}` endpoints. Reversed bounds
return `InvalidInput` during `validate` or search. Equal endpoints with either
side excluded produce an empty set. An unbounded positive range means the field
exists. Validation permits at most 256 expression nodes and 32 levels.

Source/tag postings and ordered numeric/date postings select IDs before ranking.
NOT and empty AND need the active-ID universe. Large unions/complements can
allocate sets proportional to active records; filters do not have constant cost.
Indexes update only after WAL acknowledgement and rebuild from original payloads
on open, import and backup restore. Filtering does not change BM25 corpus-wide
IDF, RRF scoring or the vector Auto policy. It is not an authorization boundary.

## CLI expressions

Append `--where 'expression'` to `docs-search`, `docs-keywords`, `docs-hybrid` or
`docs-find`. `--source` and repeated `--tag` flags remain available.

```bash
vecnook docs-find "backup" --db data/notes --mode keyword \
  --where '(tag("rust") OR tag("notes")) AND NOT source("archive.md")'
vecnook docs-find "backup" --db data/notes --mode keyword \
  --where 'number("file_bytes") >= 100 AND date("modified") >= "2026-10-01"'
```

NOT binds before AND, which binds before OR. Parentheses override precedence.
Functions and operators ignore ASCII case. `source("...")` and `tag("...")`
are equality/membership predicates. `number("field")` and `date("field")`
require `=`, `==`, `!=`, `<`, `<=`, `>` or `>=`. Numeric literals must parse as
finite f64 values. Dates use quoted `YYYY-MM-DD`. Strings use JSON escapes,
including Unicode surrogate pairs. Input is at most 16 KiB; malformed input
fails the query and never falls back to an unfiltered search.

To populate file metadata from a Markdown folder:

```bash
vecnook docs-sync ./my-markdown --db data/notes --record-metadata
```

This opts into `numbers.file_bytes` (whole UTF-8 file bytes, shared by its chunks)
and `dates.modified` (the file modification date in UTC). Later syncs infer the
setting from managed documents when the flag is omitted. An attribute change
updates that source; current sync re-embeds it even if only the file date changed.
`--no-record-metadata` explicitly removes these attributes. Default new imports
have no attributes. Files that produce no chunks have no searchable attributes.

## Payload and reader compatibility

Documents without numeric/date fields retain byte-identical `VDOC1:` encoding.
Documents with attributes use `VDOC2:`: the same base fields/tags, then a u32
numeric count, length-prefixed keys and little-endian f64 values, followed by a
u32 date count, keys and u32 `year*10000+month*100+day` values. Decode rejects
duplicate fields, nonfinite numbers, invalid dates, count/size violations and
trailing bytes.

Vecnook 1.2 reads both versions, including actual 1.1 snapshot/WAL/cache fixtures.
The snapshot, WAL, collection header and graph-cache formats are unchanged.
**1.1 and older document readers cannot decode VDOC2.** Adding attributes is an
explicit opt-in. Keep a pre-upgrade backup for downgrade, or remove all attributes
with 1.2 and checkpoint/export into a separate collection before using an older
document reader. The ordinary source/tag-only workflow keeps VDOC1.

Checkpoint with 1.2 before opening its VDOC1 data in an older binary. In 1.2,
inactive payloads are reclaimed during WAL replay; a long replacement-heavy WAL
can exceed an older reader's historical-payload byte budget even when the current
snapshot fits. A 1.2 checkpoint writes the reclaimed state and clears that history.

CLI documents keep JSON schema v1 and add optional `numbers` and `dates` objects
only when nonempty. Numbers are JSON numbers; dates are ISO strings. Required
document fields and decimal-string IDs stay unchanged. Consumers must ignore
additional fields under the existing [compatibility contract](compatibility.md).

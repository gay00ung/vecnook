# Help improve Vecnook

Try the [prepared demo](quickstart.md), then connect a Markdown folder or a Rust
application. Share the furthest step you completed, which result you expected,
and what blocked you. The repository's examples demonstrate integration; external
applications and repeat usage are measured separately.

- [Report a bug](https://github.com/gay00ung/vecnook/issues/new?template=bug_report.yml)
- [Propose a feature](https://github.com/gay00ung/vecnook/issues/new?template=feature_request.yml)
- [Share usage feedback](https://github.com/gay00ung/vecnook/issues/new?template=usage_feedback.yml)
- [Ask a question](https://github.com/gay00ung/vecnook/issues/new)

Use a small authored example, full version/commit, OS, Rust version, dimensions,
metric, counts, filters and error output. A search-quality report needs both HNSW
and exact results for the same independent questions. A storage report should
identify successful acknowledgements and operations that returned errors.
`doctor` output can describe the affected state without opening it for recovery.
The [operations guide](operations.md) explains that distinction.

Maintainers first reproduce the report, classify its impact, link the regression
check and fix, and request confirmation on a released version. Data loss,
incorrect exact results, filter leakage and failed restores take priority over
new features. Performance requests need repeatable release measurements and an
application outcome; use [the measurement method](text-benchmarks.md).

There is no built-in telemetry. The database does not send vectors, document
text or usage events to maintainers. The optional Markdown client sends input to
the local Ollama instance you selected. GitHub reports are public and voluntary.
Sharing an app link does not grant permission to publish a case study; attribution
permission is a separate optional field.

We want to observe installation, first search, data integration and later reuse.
Downloads and stars cannot establish those outcomes. No external adoption result
is claimed here; participants' reports must support any future case study.

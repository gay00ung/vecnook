# Searching within a project

Attach project names or tags to each document chunk. Apply the project filter before choosing the nearest candidates so the results contain only the requested project.

A small eligible set is often cheaper to scan exactly. For larger sets, the graph can traverse records from other projects while only returning eligible records. Post-filtering a small global Top-K can leave too few results.

Full Top-K count is a completeness signal. It does not prove that approximate graph search returned the same nearest neighbors as exact search. Measure Recall@K against an exact oracle separately.

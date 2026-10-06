# Choosing graph search settings

efSearch controls the candidate pool explored during a query. A larger pool usually finds more nearby vectors but requires more distance calculations. Measure the tradeoff using your own embeddings and independent queries.

M bounds graph neighbor connections. efConstruction controls the candidate pool while inserting a vector. Raising these settings can improve connectivity while increasing indexing time and graph memory.

Start with the default Auto strategy. It scans small collections exactly and uses graph search for larger collections. Increase efSearch when a measured Recall@K target is not met.

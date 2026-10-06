# Embedding model compatibility

An embedding model converts text into a numeric vector. Document chunks and queries must use the same model, model version, dimensionality, and compatible task prompts.

Two models can produce vectors of the same dimension while assigning different meanings to those coordinates. Matching the dimension alone is insufficient. Store a model identity with each collection and rebuild when the model changes.

Cosine distance compares vector direction. Squared L2 compares coordinate differences, and negative inner product rewards a larger dot product. Choose the metric expected by the model and keep it fixed for the collection.

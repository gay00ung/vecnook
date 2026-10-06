# Updates, deletion, and backups

Replacing an ID appends a new vector node and marks the old node deleted. Deleting an ID makes it absent from search while its old node can still help graph traversal.

Repeated updates and deletes accumulate tombstones. Compaction rebuilds the graph from active records and reclaims those nodes. It temporarily holds another index, so plan for additional peak memory.

Checkpointing saves current records and a disposable graph cache, then truncates the log. Use the backup API to create an independently openable copy in a new directory. Copying individual live files can produce an inconsistent backup.

# Recovering a local database

Vecnook synchronizes each successful mutation to its write-ahead log before acknowledging it. Closing the process without checkpointing preserves those acknowledged changes. Opening replays complete log frames after the last snapshot.

An incomplete final frame is removed at recovery. A complete frame with an invalid checksum fails opening, because silently skipping damaged data could lose an acknowledged write. Keep a backup before investigating corrupted files.

After an I/O error, close the handle, reopen the database, and inspect the affected IDs. A failed operation may have reached storage before the error. The poisoned handle prevents subsequent writes from hiding this uncertainty.

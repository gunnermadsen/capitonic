# Online-source data path certification

A data path is one online-source dataset and its realtime or backfill collection path, ingester strategy, database table, drain strategy, and canonical SSD Parquet destination. Certify one database table at a time. Weather observations and Polymarket CLOB orderbooks are in scope. Internally generated bot, trading, position, fill, order, accounting, job, and model-training records are not data paths under this definition; do not add a drain for them as part of this program.

The [data-path certification tracker](../packages/market-data-ingester/docs/data-path-certification.md) is the single inventory and status record. This process does not create another strategy registry, job ledger, worker type, file root, or storage schema. Collection uses the existing registered `RealtimeWorkerStrategy` or `BackfillWorkerStrategy` and shared ingester API. Drain uses the existing registered strategy, `POST /drains`, job and receipt contracts, and the canonical SSD Parquet layout. Source removal is guarded by the applicable committed database contract.

## Requirements for each table

Mark a path certified only when all seven requirements have evidence:

1. **Working tests:** focused regression coverage exercises collection, table persistence, drain success, and failure or cancellation without source loss.
2. **Drain scheduling:** its registered drain strategy can always be requested through the shared drain API. A period without cutoff-eligible rows does not mean scheduling is broken.
3. **Collection scheduling:** its realtime or backfill strategy can be scheduled or enabled through the shared ingester API and worker contract. It need not support both modes unless both belong to that source path.
4. **Safe writes:** a bounded real collection demonstrates correctly scoped, idempotent or conflict-safe persistence in the intended table without damaging other rows.
5. **Drain integrity:** a real **move** job writes to the canonical SSD location, validates schema and source-row content parity before publication and removal, records a durable receipt, and proves that only the verified source range was removed. Empty output when source rows exist, all-null rows, mismatched content, and corrupt Parquet must prevent removal of the affected source. Cancellation or failed verification stops further removal and exposes an error in job state and worker logs; any chunks already verified and removed retain their receipts. A copy-only export is not move certification.
6. **Instrumentation:** strategy and associated worker health and progress are visible through the existing metrics and logs, including collection state and drain progress, success, failure, and rows or bytes moved. The market data pipeline dashboard shows these signals in its drain row.
7. **Short buffer:** the configured drain buffer is no more than five days, with a smaller value whenever the live lookback and restart hydration contract permits it. Use zero days for paths whose database history is not needed after collection. A source change alone is not evidence that the selected deployed image uses that buffer.

Preserve each path's existing source semantics, natural key, table schema, API contracts, and canonical Parquet storage schema. Do not use historical training demand to justify database retention. Check any live database reader and restart cursor before reducing a buffer; preserve the narrow lookback actually required for operation.

## Certification order

For each path, follow this order:

1. Identify which of the seven requirements are missing. Record its online source, ingestion mode and strategy, target table, drain strategy, SSD destination, current buffer, live readers, and any backfill or realtime failure.
2. Implement only the missing requirements through the existing contracts. Use the approved migration process for any required schema or source-removal change.
3. Run focused tests and record their result. Keep a failure or parity mismatch from reaching source removal.
4. Determine certification against all seven requirements. Record each passing check and any remaining failure in the tracker. Do not mark the path certified on code tests or old drain receipts alone.
5. For a path that passes code checks, build and deploy only its affected image to the existing ingester master and worker runtime. Validate a bounded real collection and move drain under that selected image, including the database state, SSD file and receipt, API job outcome, and metrics. If live evidence fails, keep the path pending and investigate before another removal.
6. Tag the exact source commit for each built and deployed image with the repository's image provenance tag, then record the immutable image identity, embedded revision, collection job or realtime evidence, drain job and receipt IDs, and validation result in the tracker.

After each path reaches certification, report the table and source, the seven check results, buffer, selected image and tag, collection and move job IDs, verified rows and bytes, database removal evidence, dashboard or log evidence, and the updated certified/total count. If it remains pending, name the failed check and do not increase the count. Historical successful drains are evidence of removal, but certification requires the complete path to pass under the selected image.

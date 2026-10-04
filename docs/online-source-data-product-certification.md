# Drainable data product certification

A drainable data product is any system table that accumulates data and has a proven safe archive boundary, its existing writer, registered drain strategy, and canonical SSD Parquet destination. Online-source, internally generated trading, accounting, operational, and job records are eligible for assessment. Eligibility does not authorize removal: preserve evidence required by trading, accounting, recovery, references, and active jobs. Certify one table and its exact row or payload removal scope at a time. Do not change writer or ingestion architecture to qualify a table.

The [data-product certification tracker](../packages/market-data-ingester/docs/data-product-certification.md) is the single inventory and status record. This process does not create another strategy registry, job ledger, worker type, file root, or storage schema. Online-source collection uses the existing registered `RealtimeWorkerStrategy` or `BackfillWorkerStrategy` and shared ingester API. Internal products retain their existing application writer; no artificial ingestion strategy or backfill is required. Drain uses the existing registered strategy, `POST /drains`, job and receipt contracts, and the canonical SSD Parquet layout. Source removal is guarded by the applicable committed database contract.

## Requirements for each table

Mark a product certified only when all seven requirements have evidence:

1. **Working tests:** focused regression coverage exercises collection, table persistence, drain success, and failure or cancellation without source loss.
2. **Drain scheduling:** its registered drain strategy can always be requested through the shared drain API. A period without cutoff-eligible rows does not mean scheduling is broken.
3. **Collection or writer:** realtime or backfill collection is schedulable through the shared ingester API, or the existing internal application writer demonstrates fresh correctly scoped persistence. No new writer is introduced. Both collection modes are required only when both belong to the product.
4. **Safe writes:** a bounded real collection demonstrates correctly scoped, idempotent or conflict-safe persistence in the intended table without damaging other rows.
5. **Drain integrity:** a real **move** job writes to the canonical SSD location, validates schema and source-row content parity before publication and removal, records a durable receipt, and proves that only the verified source range was removed. Empty output when source rows exist, all-null rows, mismatched content, and corrupt Parquet must prevent removal of the affected source. Cancellation or failed verification stops further removal and exposes an error in job state and worker logs; any chunks already verified and removed retain their receipts. A copy-only export is not move certification.
6. **Instrumentation:** strategy and associated worker health and progress are visible through the existing metrics and logs, including collection state and drain progress, success, failure, and rows or bytes moved. The market data pipeline dashboard shows these signals in its drain row.
7. **Short buffer:** newly certified drain scopes use a 12-hour cutoff and drain eligible data within 24 hours, including window rounding and scheduling delay. Document protected evidence and any operationally necessary exception; do not claim whole-table retention when only a subset or payload is drained. Existing certifications with longer buffers retain their historical evidence but do not establish compliance with this stricter requirement. Confirm the selected deployed image enforces the buffer. Retry and restart verification must prove source safety and receipt continuity.

Preserve each product's existing source semantics, natural key, table schema, API contracts, and canonical Parquet storage schema. Do not use historical training demand to justify database retention. Check any live database reader and restart cursor before reducing a buffer; preserve the narrow lookback actually required for operation.

## Certification order

For each product, follow this order:

1. Identify which of the seven requirements are missing. Record its source or application writer, ingestion mode and strategy when applicable, target table, drain strategy, SSD destination, current buffer, live readers, and any backfill or realtime failure.
2. Implement only the missing requirements through the existing contracts. Use the approved migration process for any required schema or source-removal change.
3. Run focused tests and record their result. Keep a failure or parity mismatch from reaching source removal.
4. Determine certification against all seven requirements. Record each passing check and any remaining failure in the tracker. Do not mark the product certified on code tests or old drain receipts alone.
5. For a product that passes code checks, build and deploy only its affected image to the existing ingester master and worker runtime. Validate a bounded real collection or internal write and move drain under that selected image, including the database state, SSD file and receipt, API job outcome, and metrics. If live evidence fails, keep the product pending and investigate before another removal.
6. Tag the exact source commit for each built and deployed image with the repository's image provenance tag, then record the immutable image identity, embedded revision, collection job or realtime evidence, drain job and receipt IDs, and validation result in the tracker.

After each product reaches certification, report the table and source, the seven check results, buffer, selected image and tag, collection and move job IDs, verified rows and bytes, database removal evidence, dashboard or log evidence, and the updated certified/total count. If it remains pending, name the failed check and do not increase the count. Historical successful drains are evidence of removal, but certification requires the complete path to pass under the selected image.

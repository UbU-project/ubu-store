# Atomic batch admission

A batch is an ordered set of canonical Object and Log writes. `queries::admit_batch` opens one transaction and returns one `BatchResult` for each `BatchWrite`, in request order. Every new object, update, Log and mutation receipt commits together or rolls back together. An empty batch returns an empty vector and writes nothing.

The invariant remains: **only the outer public writer owns commit or rollback**. Individual writers and the batch share prepared object/Log admission, envelope validation, preconditions, replay, canonical payload checks and ledger recording. Transaction plumbing remains crate-private. Callers supply the existing pool handle and domain records, not a transaction or connection.

## Why this exists

The ticket's prototype showed the partial structure left by sequential calls when a third child was invalid:

```text
PROBE[today] third child rejected: true
PROBE[today] before (objects, logs, ledger) = (1, 0, 1)
PROBE[today] after  (objects, logs, ledger) = (3, 1, 4)
PROBE[today] origin status = active
PROBE[today] Container admitted = 0
PROBE[today] => two orphan child Tasks, a task_decomposed Log entry, no Container,
PROBE[today]    and an origin the operator still sees as active work.
```

The same failure in one transaction leaves the original state intact:

```text
PROBE[rollback] batch rejected: true
PROBE[rollback] reason: invalid JSON payload: UBU-D0227: task status `moot` requires moot_reason_code
PROBE[rollback] before (objects, logs, ledger) = (1, 0, 1)
PROBE[rollback] after  (objects, logs, ledger) = (1, 0, 1)
PROBE[rollback] origin status = active
```

A valid batch admits the entire replacement and can be replayed without new writes:

```text
PROBE[batch] before (objects, logs, ledger) = (1, 0, 1)
PROBE[batch] after  (objects, logs, ledger) = (5, 1, 7)
PROBE[batch] results = 6
PROBE[batch]   Log log_…900a task_decomposed
PROBE[batch]   Task task_…b2c2 v1 active
PROBE[batch]   Task task_…0c60 v1 active
PROBE[batch]   Task task_…b380 v1 active
PROBE[batch]   Container container_…62b8 v1 active
PROBE[batch]   Task task_…cf9b v2 moot
PROBE[batch] replay results = 6, counts unchanged = (5, 1, 7)
PROBE[batch] origin version after replay = 2
```

A stale origin precondition on the last write also rolls back the earlier writes:

```text
PROBE[conflict] batch rejected: true
PROBE[conflict] reason: precondition failed for `task_…9f9f`: expected v1, actual v2
PROBE[conflict] before (objects, logs, ledger) = (1, 0, 2)
PROBE[conflict] after  (objects, logs, ledger) = (1, 0, 2)
```

These are all four probe blocks supplied by P1B-37a (its documentation list refers to “three”). The regression tests reproduce their counts and outcomes using fully synthetic data.

## Envelopes, order and replay

`mutation_envelopes` has primary key `(origin_device_id, idempotency_key)`. A receipt identifies one `result_object_id`, so each write keeps its own envelope and key. No batch identity, new ledger table or shared envelope is introduced.

Writes apply in request order. A later write's observed-version precondition sees earlier writes in the same transaction. For example, a child may require the preceding child at version 1, and the final origin mutation may require its pre-decomposition version. Any failed validation, stale precondition or ledger insert aborts the complete batch.

Two writes carrying the same Device/key pair within one batch are an error, even if their payloads are identical. The batch checks this explicitly: the ordinary preparation helper would otherwise interpret an identical repeated key as a valid replay before SQLite's primary key could reject it. Existing single-write replay behaviour is unchanged.

Replay is per write. When the original request was submitted as one batch, its receipts were committed all together or not at all. Repeating that batch returns the ordinary writers' current-state results without advancing versions or adding receipts. This is not historical result storage. As with the individual writers, payload equality and target-table checks govern replay. Keys already admitted through other calls can still replay individually; atomicity covers the new writes in the current batch, not a fabricated global batch receipt.

`BatchWrite::Object` carries a `MutationEnvelope` and `NewObjectRecord`; `BatchWrite::Log` carries an envelope and `NewLogRecord`. `BatchResult::as_object` and `as_log` provide borrowed access to the corresponding admitted result. Both enums are `Debug + Clone` and are also exported through `api::admission`.

Six writes do not mean six new object rows: the decomposition-shaped batch inserts three children, one Container and one Log, then updates the existing origin. Object count increases by four, Log count by one, and ledger count by six.

## Container boundary

The new core Container records the exact origin Task version, mutation Log, user intent, ordered Task references, split points, lifecycle and provenance. Optional `superseded_by_task_ref` is present exactly when status is `superseded`. `segments()` derives exclusive ranges: `[2]` over four children produces `0..2` and `2..4`. `completion_state` returns a result so an incomplete child-state list is an error; complete means every child is completed or moot. Completion is never stored.

The schema and core define and validate this record; batch admission retains ordinary store validation. This ticket creates no Container through an operator endpoint and changes no planning, projection, decomposition or undo behaviour. Those operations belong to the subsequent P1B-37 work under DESIGN §9.4 and UBU-D0278.

## Known limits

1. **No partial success.** A batch is all or nothing by design. A caller wanting best-effort behaviour must issue separate writes and handle the partial state itself.
2. **No nested batches.** `admit_batch` opens a transaction; calling it from inside another writer is not supported and is not guarded against.
3. **No candidate writes in a batch.** `admit_advisory_candidate` keeps its own writer. Composing a candidate decision into a batch would need the candidate lifecycle checks too, and nothing needs it yet.
4. **Batch size is unbounded.** Nothing caps how many writes one transaction may hold. A decomposition is a handful; an import is not, and would need its own judgment.
5. **The Container type is defined but unused.** `P1B-37` is the first thing that creates one. Nothing in this ticket plans, projects or displays a Container.
6. **Split points are validated in `ubu-core`, not in JSON Schema.** The upper bound depends on `items`' length, which the schema cannot express.
7. **Container `status` is not enforced by store admission.** Lifecycle-status validation in `ubu-store` covers Tasks only; the Container's status is enforced by its schema and by `Container::validate`.


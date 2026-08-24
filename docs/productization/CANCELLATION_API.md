# DataPack cooperative cancellation API

Status: **IMPLEMENTED** and **CERTIFIED** locally and in hosted P4 CI run
`32709843703`, where all 18 required jobs passed. Python `KeyboardInterrupt`
conversion and CLI signal integration are **DEFERRED**.

## Architecture

The Rust Application layer owns one explicit cancellation contract:

```text
Application operation
    + ProgressObserver       (observation)
    + CancellationToken      (control)
        -> cooperative checkpoints in existing execution paths
        -> typed OperationError::Cancelled
```

Cancellation does not create another compression, decompression, analysis,
validation, comparison, or benchmark engine. It adds cheap checkpoints to the
existing paths. Progress callback return values and callback exceptions never
request cancellation.

## Rust contract

`datapack::application::CancellationToken` is a cloneable, thread-safe handle
around shared `Arc<AtomicBool>` state. `cancel()` performs an idempotent,
monotonic request; `is_cancelled()` queries it. Release/Acquire atomic ordering
publishes and observes the request without locks. A token has no reset method.
Callers should normally create one token per logical operation, but deliberate
sharing is supported; passing an already-cancelled token pre-cancels every
subsequent controlled operation that receives it.

The existing simple and `*_with_progress` functions remain unchanged. Each of
the six Application operations additionally has a `*_with_control` function:

- `analyze_with_control`;
- `compress_with_control`;
- `decompress_with_control`;
- `validate_with_control`;
- `compare_with_control`; and
- `benchmark_with_control`.

They accept an `OperationControl`, which independently carries an optional
progress observer and an optional cancellation token, and return
`OperationResult<T>`. This avoids separate progress-only, cancellation-only,
and combined permutations while preserving all prior entry points.

The new non-exhaustive `OperationError` is either `Cancelled` or
`Failed(DatapackError)`. The existing, exhaustively matchable `DatapackError`
does not gain a variant, so existing callers are not forced to change matches.
Cancellation is therefore typed without introducing a breaking change to the
established error enum.

At a checkpoint, an already-requested cancellation wins. If an ordinary
failure has already occurred before another checkpoint can observe the token,
that original error remains `OperationError::Failed`; DataPack does not rewrite
unrelated I/O, format, corruption, or validation failures as cancellation.

## Progress relationship

Every normal progress delivery checks cancellation immediately before and
after the synchronous observer call. This makes the following pattern
deterministic without giving the callback a special return protocol:

```rust,no_run
use datapack::application::{
    self, CancellationToken, CompressRequest, OperationControl, ProgressEvent,
};

let token = CancellationToken::new();
let observer_token = token.clone();
let mut observer = move |event: &ProgressEvent| {
    if event.completed_bytes > 8 * 1024 * 1024 {
        observer_token.cancel();
    }
};

let result = application::compress_with_control(
    CompressRequest::new("input.bin", "output.dpack"),
    OperationControl::new()
        .with_progress(&mut observer)
        .with_cancellation(token),
);
```

Accepted cancellation never emits whole-operation
`Finalizing/Completed`. Progress already delivered remains factual and is not
reset. P3's Rust observer-panic behavior and Python unraisable-callback policy
remain unchanged and are not cancellation mechanisms.

## Transaction and cleanup semantics

Compress and Decompress use the existing sibling `TempOutput`. Cancellation
before commit returns `Cancelled`; dropping the transaction removes its owned
temporary output by default and does not create or replace the requested final
path. If a valid destination already exists, it remains unchanged.

`keep_partial` is unchanged. When explicitly enabled, cancellation may retain
the operation-owned sibling `.partial` artifact under the existing failure
policy. It never renames that artifact to the final destination and never
claims that it is a valid completed archive or restored output.

There is a final cancellation checkpoint immediately before transactional
commit. If it observes the request, cancellation wins and no final replacement
occurs. Once commit succeeds, success wins; a later request cannot delete the
committed output or retroactively turn the operation into cancellation.
Terminal success progress is consequently emitted after commit without a
post-commit cancellation check.

## Checkpoint granularity

Cancellation is cooperative and is not promised to be immediate.

- Analyze checks before work, around its bounded input/sample stage, and
  before returning its report. It does not scan extra input.
- V1 raw-zstd Compress and Decompress check at their natural 64 KiB streaming
  loop boundaries and around planning, writes, flushing, and commit. Structured
  v1 in-memory transforms remain coarser and check around major stages.
- V2 Compress checks around setup, ordered result receipt, every ordered chunk
  write/progress delivery, table finalization, flushing, and commit. The
  reader/worker/ordered-writer architecture, bounded channels, and output order
  remain unchanged. Already-running bounded worker work may finish during
  scoped teardown; uncommitted results are discarded and the final archive is
  not committed.
- V2 Decompress checks at each verified chunk boundary and before commit.
- Validate checks around setup, source hashing blocks, v1 stream work, and v2
  chunk validation. Cancellation never produces a `valid` or `invalid` report.
- Compare checks at bounded snapshot/hash reads and between measured
  compression, decompression, validation, and cleanup stages. It returns no
  complete comparison report after cancellation is accepted.
- Benchmark checks between planning and every planned suboperation/run. V2
  suboperations use the same chunk checkpoints. Some legacy in-memory helper
  calls are coarse; once cancellation is observed, later benchmark work and the
  complete report are suppressed.

No checkpoint is inserted per byte or row. Cancellation latency is bounded by
the current natural unit of work, and legacy in-memory codec stages may take
longer to yield than streaming or v2 chunk paths.

## Python contract

Python exports `datapack.CancellationToken` with `cancel()` and the read-only
boolean property `is_cancelled`. Analyze, Compress, Decompress, Validate, and
Compare accept the optional keyword-only argument `cancellation=` in addition
to the P3 `progress=` callback:

```python
import datapack

token = datapack.CancellationToken()

def on_progress(event):
    if event.completed_bytes > 8 * 1024 * 1024:
        token.cancel()

try:
    datapack.compress(
        "input.bin",
        "output.dpack",
        progress=on_progress,
        cancellation=token,
    )
except datapack.CancelledError:
    pass
```

`datapack.CancelledError` is a distinct subclass of `DataPackError` mapped
only from Rust `OperationError::Cancelled`; other exception mappings remain
unchanged. A Python progress callback exception is still reported through
`sys.unraisablehook`, disables later callbacks for that operation, and lets the
Rust operation continue.

Long Rust work still runs under `Python::detach`. The native Python token owns
a clone of the same Rust atomic state, so another Python thread can call
`cancel()` without a polling thread and without waiting for the long operation
to regain the GIL. Progress callbacks safely reattach as established in P3.

## Frontend status

- Explicit Rust and Python cancellation: **IMPLEMENTED** and **CERTIFIED**
  locally and in hosted CI run `32709843703`.
- Python callback-driven explicit `token.cancel()`: **CERTIFIED** locally and
  in hosted CI.
- Cancellation from another Python thread while detached Rust work runs:
  **CERTIFIED** locally and in hosted CI.
- Python `KeyboardInterrupt`/Ctrl+C conversion: **DEFERRED**. The binding does
  not install signal handlers or claim portable interrupt conversion.
- CLI Ctrl+C integration: **DEFERRED**. Current process-level termination was
  audited, but P4 adds no signal dependency and makes no cleanup guarantee for
  an externally terminated process beyond operating-system behavior.
- Future Desktop integration: the cloneable token can be stored with an
  operation handle and invoked from a UI/IPC thread; Desktop/Tauri remains
  **DEFERRED**.

No async runtime, scheduler, background service, unsafe thread termination, or
new dependency is introduced.

## Performance evidence

P4 uses atomic loads only at natural checkpoints. The local deterministic
4 MiB v2 workload (256 KiB chunks, one worker, at most two chunks in flight)
recorded 16 chunks. A token installed but never cancelled produced an archive
byte-for-byte equal to the no-control archive. Its debug-profile timings are
recorded as **OBSERVATIONAL** in the Productization report; there is no
wall-clock CI threshold.

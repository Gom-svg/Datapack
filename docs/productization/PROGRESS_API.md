# DataPack progress API

Status: **IMPLEMENTED** and **CERTIFIED** locally and in hosted CI in P3;
evidence is recorded in the main Productization report. P4 now composes a
separate explicit cooperative cancellation control beside this unchanged
progress contract.

## Architecture

The Rust Application API is the single product progress source:

```text
DataPack execution
    -> application ProgressEvent facts
        -> Rust caller / CLI / Python callback / future IPC adapter
```

Progress is observational. No codec, planner, validation, archive, or Python
execution path depends on an observer being present. The simple Application
functions and their existing `*_with_progress` counterparts execute the same
operations.

## Rust contract

`ProgressEvent` contains:

- `operation: OperationKind`;
- `phase: ProgressPhase`;
- `state: ProgressState`;
- `completed_bytes: u64` and `total_bytes: Option<u64>`; and
- `completed_items: u64` and `total_items: Option<u64>`.

The exact operation values are `Analyze`, `Compress`, `Decompress`, `Validate`,
`Compare`, and `Benchmark`. The exact phases are `Analyzing`, `Planning`,
`ReadingInput`, `Hashing`, `Compressing`, `WritingArchive`, `ReadingArchive`,
`Decompressing`, `WritingOutput`, `Validating`, `Comparing`, `Benchmarking`,
`CleaningUp`, and `Finalizing`. Phase lifecycle states are `Started`,
`Advanced`, and `Completed`.

Enums remain non-exhaustive. Their `as_str()` methods and `Serialize`
implementations provide stable snake-case adapter values without presentation
text. Events contain no paths, source values, field names, rows, hashes, or
rendered status messages.

`Finalizing/Completed` is the sole terminal-success fact. It is emitted only
after the operation has successfully produced its result and, for Compress and
Decompress, after transactional output commit. An error emits no terminal
success event. Earlier phase completion means only that phase completed; for
example, `WritingOutput/Completed` does not claim that the final path is
committed.

## Counters and percentage

Counters are absolute integer facts. Within one active phase lifecycle they are
monotonic, and a completed value never exceeds its supplied total. Totals are
`None` when they are unknown or not meaningful. Item counters currently mean
ordered chunks for v2 Compress and Decompress; phases without a truthful item
unit use zero with an unknown item total.

`ProgressEvent::percentage()` is the canonical derivation from byte counters.
It returns `None` for an unknown total and for a zero-byte phase that has not
completed. A successfully completed zero-byte phase returns `100.0`. Sampling
and bounded comparison completion can truthfully complete below 100 percent of
the source; phase completion and percentage are distinct facts.

Elapsed time, throughput, ETA, and wall-clock throttling are presentation
concerns and are not fields in the contract. Core I/O snapshots use a
deterministic 8 MiB byte cadence. V2 uses its natural chunk-commit cadence.
There is no correctness test based on wall-clock duration.

## Operation granularity

- Analyze emits a coarse `Analyzing` lifecycle. Its total is the bounded sample
  scope, not the whole source size. It does not imply that an intentionally
  sampled file was fully scanned.
- V1 Compress reports bounded planning/sample facts, deterministic I/O
  snapshots where streaming is available, and coarse in-memory encoding.
- V2 Compress advances only after a worker result is restored to input order
  and its compressed chunk is written to the transactional archive. Worker
  completion alone is not product completion. Bounded in-flight and ordered
  writer behavior are unchanged.
- V1 Decompress reports bounded archive reads or restored-output writes where
  available. In-memory decode remains coarse.
- V2 Decompress advances after each verified decoded chunk is written in order
  to the transactional output. Terminal success follows final commit.
- Validate emits a truthful coarse archive-validation lifecycle. Archive-only
  and against-source checks retain their full semantics; no unsupported
  substage percentage is invented.
- Compare emits a truthful coarse lifecycle over the selected comparison scope.
  A bounded Quick comparison does not report the unexamined source suffix as
  completed work.
- Benchmark retains its existing typed high-level phase transitions. Callback
  execution can perturb caller-observed timing, so silent `benchmark()` remains
  the preferred path for least-contaminated measurements.

## Callback behavior

Rust observers are mutable and synchronous. They execute in delivery order on
the operation's calling thread and must return before execution continues.
Delivery frequency beyond lifecycle, deterministic I/O boundaries, and natural
chunk boundaries is not a stable event-count promise.

The existing Rust callback is infallible by type. If it panics, normal Rust
unwinding applies. Transaction guards prevent a panic before commit from
publishing a partial final output. A panic during a terminal post-commit event
cannot undo an output that was already committed successfully. Callers must not
use panic as operation control.

Python accepts an optional keyword-only `progress` callable on `analyze`,
`compress`, `decompress`, `validate`, and `compare`. It receives an immutable
`datapack.ProgressEvent` with snake-case `operation`, `stage`, and `state`, the
same counters/totals, Rust-derived `percentage`, and a `terminal` helper.

Long-running Rust work runs with the Python interpreter detached. Each callback
safely reattaches, invokes Python synchronously on the calling thread, and then
detaches again. If a Python callback raises, DataPack reports the exception via
Python's unraisable-exception hook, disables that observer for the rest of the
operation, and lets the Rust operation finish. This explicit isolation policy
keeps progress observational and is not cancellation. Non-callable values are
rejected before work starts.

## CLI and future adapters

Analyze, Compress, Decompress, Validate, and Compare pass the same typed events
to the CLI observer. Human progress is written only to stderr when stderr is a
terminal; redirected output and JSON/report stdout remain uncontaminated. The
legacy Benchmark CLI and Application adapter consume the same internal typed
Benchmark events, while the Application API presents them as `ProgressEvent`.

The serializable facts and stable string identifiers can be bridged to future
Desktop IPC without exposing codec or archive internals. P4 composes a
separate cooperative `CancellationToken` beside the observer through
`OperationControl`; callback return values and exceptions still cannot cancel
an operation. See `CANCELLATION_API.md`. No task runtime or async runtime was
added.

## Current limitations

- Analyze, Validate, Compare, and several v1 in-memory transforms are coarse;
  P3 does not redesign them merely to manufacture intermediate percentages.
- Progress callbacks are synchronous and can add caller wall time. Python
  callbacks additionally pay interpreter reattachment and object-allocation
  cost.
- Event cadence is useful but not a stable telemetry sampling protocol.
- No ETA model is implemented. Cooperative cancellation is a separate P4
  control contract and does not change these progress facts.

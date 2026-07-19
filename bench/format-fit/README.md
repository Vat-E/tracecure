# format-fit

Benchmark harness answering a single question:

> Should Tracecure's M1 storage layer build on `arrow-rs` + `parquet`, or
> should we pivot to a different format?

**Answered: yes, adopt parquet.** See
[`docs/m1-format-fit-report.md`](../../docs/m1-format-fit-report.md) for the
measured numbers and [ADR-001](../../docs/decisions/ADR-001-m1-storage-format.md)
(revised by [ADR-002](../../docs/decisions/ADR-002-m1-writer-knobs-revision.md))
for the decision.

## Run

All flags have defaults matching the original spike, so a bare run reproduces
it:

```
cargo run --release --manifest-path bench/format-fit/Cargo.toml
```

To vary a parameter (note: plain digits — `10_000_000` is a Rust literal, not
valid CLI input, and clap rejects it):

```
cargo run --release --manifest-path bench/format-fit/Cargo.toml -- \
  --records 1000000 \
  --output ./target/format-fit/corpus.parquet \
  --seed 42
```

Defaults: `--records 10000000`, `--seed 42`, `--query-iters 5`,
`--batch-rows 1000000`, output `./target/format-fit/corpus.parquet`.
Run with `--help` for the full list, including the ADR-002 v2 knobs
(`--batch-rows`, bloom-filter toggles).

A full 10M-record run adds roughly 2–5 GB under `./target/format-fit/`.
Clean with `rm -rf ./target/format-fit/`.

## Output

Plain text report on stdout: throughput, file size, write amplification,
footer size, and two label-filter timings (Map column vs flattened column).

## Status — kept, not dead

This crate is **excluded from the workspace** (root `Cargo.toml`,
`exclude = [...]`), so it costs normal builds nothing and must not be
depended on from any shipping crate.

The M1 question it was built for is answered, but it is deliberately retained
because two open items point at it:

- [ADR-002](../../docs/decisions/ADR-002-m1-writer-knobs-revision.md) requires
  that any future bloom filter be enabled per-column with an explicit `ndv` —
  a value never measured. The spike used the parquet default, which is
  precisely why the file doubled in size.
- ADR-002 also defers a label index to M3, whose payoff has to be measured
  against the ~1.4 s / 10M-row `Map` scan baseline this harness established.

Re-running it is the only way to produce comparable numbers for either.

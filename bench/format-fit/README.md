# format-fit

One-shot benchmark answering a single question:

> Should Sift's M1 storage layer build on `arrow-rs` + `parquet`, or should we pivot to a different format?

See the M1 plan in `docs/superpowers/plans/2026-04-23-m1-format-fit-spike.md` for the full protocol and acceptance thresholds.

## Run

```
cargo run --release --manifest-path bench/format-fit/Cargo.toml -- \
  --records 10_000_000 \
  --output ./target/format-fit/corpus.parquet \
  --seed 42
```

Adds roughly 2–5 GB to `./target/format-fit/`. Clean with `rm -rf ./target/format-fit/`.

## Output

Plain text report on stdout with throughput, file size, write amplification, footer size, and two label-filter timings (Map column vs flattened column).

## Not shipped

This crate is **excluded from the workspace** (`Cargo.toml` root, `exclude = [...]`). It is a one-shot research tool. Do not depend on it from any shipping crate.

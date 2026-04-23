//! `format-fit` — one-shot benchmark: is arrow-rs + parquet the right M1 format?

mod arrow;
mod gen;
mod query;

use std::path::PathBuf;
use std::time::Instant;

use clap::Parser;

#[derive(Parser, Debug)]
#[command(name = "format-fit", about = "Sift M1 format-fit spike")]
struct Cli {
    /// How many synthetic records to generate and write.
    #[arg(long, default_value_t = 10_000_000)]
    records: usize,
    /// Parquet output path. Parent dirs created if missing.
    #[arg(long, default_value = "./target/format-fit/corpus.parquet")]
    output: PathBuf,
    /// RNG seed for deterministic runs.
    #[arg(long, default_value_t = 42)]
    seed: u64,
    /// How many times to run each query (timings are averaged).
    #[arg(long, default_value_t = 5)]
    query_iters: u32,
}

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    println!("format-fit — Sift M1 format-fit spike");
    println!("records    = {}", cli.records);
    println!("output     = {}", cli.output.display());
    println!("seed       = {}", cli.seed);
    println!("query_iters= {}", cli.query_iters);
    println!();

    // 1) Generate.
    let t0 = Instant::now();
    let records = gen::generate(cli.seed, cli.records);
    let gen_dur = t0.elapsed();
    println!("[1/4] generate: {:.2}s", gen_dur.as_secs_f64());

    // Estimate the uncompressed in-memory size. Rough lower bound for write-
    // amplification: sum of body + labels + fields_json lengths. This is not
    // perfect (it doesn't count the ~30 bytes of fixed-width columns per row)
    // but it's the honest "what the user sent us" number.
    let logical_bytes: u64 = records
        .iter()
        .map(|r| {
            (r.host.len()
                + r.app.len()
                + r.stream_id.len()
                + r.body.len()
                + r.labels.iter().map(|(k, v)| k.len() + v.len()).sum::<usize>()
                + r.fields
                    .iter()
                    .map(|(k, v)| k.len() + v.to_string().len())
                    .sum::<usize>()
                + 30) as u64
        })
        .sum();
    println!("       ~{:.2} GiB logical input", gib(logical_bytes));

    // 2) Write.
    let t0 = Instant::now();
    let stats = arrow::write_parquet(&records, &cli.output)?;
    let write_dur = t0.elapsed();
    println!(
        "[2/4] write:    {:.2}s → {:.2} MiB on disk ({} row groups)",
        write_dur.as_secs_f64(),
        stats.bytes_on_disk as f64 / 1024.0 / 1024.0,
        stats.row_groups_written
    );
    println!(
        "       throughput: {:.0} records/s, {:.1} MiB/s in, {:.1} MiB/s out",
        cli.records as f64 / write_dur.as_secs_f64(),
        logical_bytes as f64 / 1024.0 / 1024.0 / write_dur.as_secs_f64(),
        stats.bytes_on_disk as f64 / 1024.0 / 1024.0 / write_dur.as_secs_f64()
    );
    let amp = stats.bytes_on_disk as f64 / logical_bytes as f64;
    println!(
        "       write amplification: {:.2}× (on disk / logical input)",
        amp
    );

    // 3) Footer.
    let footer = arrow::footer_size(&cli.output)?;
    println!(
        "[3/4] footer:   {:.2} KiB ({:.2}% of file)",
        footer as f64 / 1024.0,
        footer as f64 * 100.0 / stats.bytes_on_disk as f64
    );

    // 4) Label filter timings, averaged.
    println!(
        "[4/4] label filter `env = v0`  (avg of {} iters)",
        cli.query_iters
    );
    let (map_dur, map_matched) = bench_query(cli.query_iters, || {
        query::label_filter_via_map(&cli.output, "env", "v0")
    })?;
    println!(
        "       via Map column:          {:>8.2} ms  → {} matches",
        ms(map_dur),
        map_matched
    );
    let (flat_dur, flat_matched) = bench_query(cli.query_iters, || {
        query::label_filter_via_flat(&cli.output, "env", "v0")
    })?;
    println!(
        "       via flattened `env` col: {:>8.2} ms  → {} matches",
        ms(flat_dur),
        flat_matched
    );
    let speedup = map_dur.as_secs_f64() / flat_dur.as_secs_f64();
    println!("       speedup of flattened over map: {:.1}×", speedup);

    // Drop the in-memory records explicitly before exit so we don't dominate
    // the final timing with a Vec<Record> destructor.
    drop(records);

    println!();
    println!("see docs/m1-format-fit-report.md for the decision once populated.");
    Ok(())
}

fn bench_query<F, E>(iters: u32, mut run: F) -> anyhow::Result<(std::time::Duration, u64)>
where
    F: FnMut() -> Result<query::QueryStats, E>,
    E: Into<anyhow::Error>,
{
    let mut total = std::time::Duration::ZERO;
    let mut last_matched = 0u64;
    for _ in 0..iters {
        let stats = run().map_err(Into::into)?;
        total += stats.duration;
        last_matched = stats.matched_rows;
    }
    Ok((total / iters, last_matched))
}

fn gib(bytes: u64) -> f64 {
    bytes as f64 / 1024.0 / 1024.0 / 1024.0
}

fn ms(d: std::time::Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

//! Op-sequence fuzzer (proptest-driven). Random op sequences biased toward the grid so
//! rectangles share corners and edges, touch, nest and cross; distances include zero,
//! sub-tolerance, huge and non-finite values. Every sequence must satisfy
//! `sawhorse_kernel::testing::check_sequence`.
//!
//! On failure the (shrunk) sequence is written to `tests/repro/pending-<test>.json`. Rename
//! it to say what it found, fix the kernel, and commit it: `tests/repro.rs` replays every
//! file there on every run.
//!
//! `cargo test --test sequences` runs a quick pass; the long run is
//! `FUZZ_CASES=20000 cargo test --test sequences -- --ignored long_fuzz` (or `just fuzz-long`).

use std::path::PathBuf;

use proptest::prelude::*;
use proptest::test_runner::{Config, TestRunner};
use sawhorse_kernel::model::Op;
use sawhorse_kernel::testing::{check_sequence, write_repro};

fn grid() -> impl Strategy<Value = f64> {
    prop_oneof![
        6 => (-8i32..8).prop_map(|k| f64::from(k) * 0.25),
        2 => -3.0..3.0f64,
        1 => Just(1e-6),
        1 => Just(0.25 + 4e-6), // inside POINT_MERGE of a grid line
    ]
}

fn distance() -> impl Strategy<Value = f64> {
    prop_oneof![
        6 => (-8i32..8).prop_map(|k| f64::from(k) * 0.25),
        3 => -3.0..3.0f64,
        1 => Just(0.0),
        1 => Just(5e-6),
        1 => Just(2e4),
        1 => Just(f64::NAN),
        1 => Just(f64::INFINITY),
    ]
}

fn normal() -> impl Strategy<Value = [f64; 3]> {
    prop_oneof![
        3 => Just([0.0, 0.0, 1.0]),
        1 => Just([1.0, 0.0, 0.0]),
        1 => Just([0.0, 1.0, -1.0]),
        1 => Just([0.0, 0.0, 0.0]),
        2 => (-1.0..1.0f64, -1.0..1.0f64, -1.0..1.0f64).prop_map(|(x, y, z)| [x, y, z]),
    ]
}

fn op() -> impl Strategy<Value = Op> {
    prop_oneof![
        1 => (grid(), grid(), grid(), normal()).prop_map(|(x, y, z, normal)| Op::NewSketch { origin: [x, y, z], normal }),
        6 => (1u64..4, grid(), grid(), grid(), grid()).prop_map(|(sketch, x, y, w, h)| Op::AddRectangle { sketch, x, y, w, h }),
        4 => (1u64..3, 1u64..5, distance()).prop_map(|(sketch, region, distance)| Op::Extrude { sketch, region, distance }),
        5 => (1u64..3, 0u32..10, distance()).prop_map(|(object, face, distance)| Op::PushPull { object, face, distance }),
    ]
}

fn sequence(max: usize) -> impl Strategy<Value = Vec<Op>> {
    prop::collection::vec(op(), 1..max).prop_map(|mut ops| {
        ops.insert(
            0,
            Op::NewSketch {
                origin: [0.0; 3],
                normal: [0.0, 0.0, 1.0],
            },
        );
        ops
    })
}

fn repro_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("repro")
}

fn run(name: &str, cases: u32, max_len: usize) {
    let mut runner = TestRunner::new(Config {
        cases,
        failure_persistence: None,
        ..Config::default()
    });
    let totals = std::cell::Cell::new((0usize, 0usize, 0usize, 0usize));
    let result = runner.run(&sequence(max_len), |ops| match check_sequence(&ops) {
        Ok(s) => {
            let (n, a, r, o) = totals.get();
            totals.set((n + 1, a + s.accepted, r + s.refused, o + s.objects));
            Ok(())
        }
        Err(e) => {
            let _ = write_repro(&repro_dir(), &format!("pending-{name}"), &e, &ops);
            Err(TestCaseError::fail(e))
        }
    });
    let (n, a, r, o) = totals.get();
    println!("{name}: {n} sequences, {a} ops accepted, {r} refused, {o} solids at the end");
    if let Err(e) = result {
        panic!(
            "{e}\nminimal failing sequence written to {}",
            repro_dir().join(format!("pending-{name}.json")).display()
        );
    }
}

#[test]
fn quick_fuzz() {
    run("quick_fuzz", 384, 24);
}

/// Long run for CI nightlies and before releases. FUZZ_CASES sets the case count.
#[test]
#[ignore]
fn long_fuzz() {
    let cases = std::env::var("FUZZ_CASES")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(20_000);
    run("long_fuzz", cases, 48);
}

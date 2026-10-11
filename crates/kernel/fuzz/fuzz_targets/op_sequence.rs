#![no_main]
//! Feeds arbitrary op sequences (NaNs, huge values, bogus indices and all) through the same
//! invariants as the proptest sequence fuzzer.

use libfuzzer_sys::fuzz_target;
use sawhorse_kernel::model::Op;
use sawhorse_kernel::testing::check_sequence;

fuzz_target!(|ops: Vec<Op>| {
    if ops.len() > 64 {
        return;
    }
    let mut seq = vec![Op::NewSketch { origin: [0.0; 3], normal: [0.0, 0.0, 1.0] }];
    seq.extend(ops);
    if let Err(e) = check_sequence(&seq) {
        panic!("{e}\n{}", serde_json::to_string(&seq).unwrap_or_default());
    }
});

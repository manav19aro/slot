use slot_achievements::{Error, GbaAchievements};

fn memory(value: u8) -> impl FnMut(u32, &mut [u8]) -> usize {
    move |address, output| {
        assert!((0x02000000..0x02040000).contains(&address));
        output.fill(0);
        output[0] = value;
        output.len()
    }
}

#[test]
fn triggers_only_after_a_false_frame_and_only_once() {
    let mut runtime = GbaAchievements::new().unwrap();
    runtime.activate(42, "0xH008000=18").unwrap();
    assert!(runtime.evaluate_frame(memory(0)).unwrap().is_empty());
    assert_eq!(runtime.evaluate_frame(memory(18)).unwrap(), [42]);
    runtime.reset_progress();
    runtime.activate(42, "0xH008000=18").unwrap();
    assert!(runtime.evaluate_frame(memory(0)).unwrap().is_empty());
    assert!(runtime.evaluate_frame(memory(18)).unwrap().is_empty());
}

#[test]
fn translates_both_ram_regions_and_save_ram_with_little_endian_values() {
    let mut runtime = GbaAchievements::new().unwrap();
    runtime
        .activate(1, "0xH000000=52_0x008000=4660_0xH048000=86")
        .unwrap();
    let read = |present: bool, address, output: &mut [u8]| {
        output.fill(0);
        if present {
            match address {
                0x03000000 => output[0] = 52,
                0x02000000 => output[..2].copy_from_slice(&[0x34, 0x12]),
                0x0e000000 => output[0] = 86,
                _ => panic!("unexpected native address {address:x}"),
            }
        }
        output.len()
    };
    assert!(runtime
        .evaluate_frame(|a, b| read(false, a, b))
        .unwrap()
        .is_empty());
    assert_eq!(
        runtime.evaluate_frame(|a, b| read(true, a, b)).unwrap(),
        [1]
    );
}

#[test]
fn failed_reads_cannot_unlock_a_zero_comparison() {
    let mut runtime = GbaAchievements::new().unwrap();
    runtime.activate(3, "0xH008000=0").unwrap();
    assert!(runtime.evaluate_frame(memory(1)).unwrap().is_empty());
    assert_eq!(
        runtime.evaluate_frame(|_, _| 0),
        Err(Error::MemoryUnavailable)
    );
    assert!(runtime.evaluate_frame(memory(1)).unwrap().is_empty());
    assert_eq!(runtime.evaluate_frame(memory(0)).unwrap(), [3]);
}

#[test]
fn out_of_range_addresses_and_panicking_readers_fail_closed() {
    let mut runtime = GbaAchievements::new().unwrap();
    runtime.activate(1, "0xH058000=0").unwrap();
    assert_eq!(
        runtime.evaluate_frame(|_, _| panic!("must not read invalid address")),
        Err(Error::MemoryUnavailable)
    );
    let mut runtime = GbaAchievements::new().unwrap();
    runtime.activate(1, "0xH008000=0").unwrap();
    assert_eq!(
        runtime.evaluate_frame(|_, _| panic!("reader failed")),
        Err(Error::MemoryUnavailable)
    );
}

#[test]
fn invalid_rule_is_reported_and_instances_do_not_share_unlocks() {
    let mut runtime = GbaAchievements::new().unwrap();
    assert!(matches!(
        runtime.activate(1, "garbage"),
        Err(Error::InvalidRule(_))
    ));
    assert_eq!(
        runtime.activate(1, "bad\0rule"),
        Err(Error::RuleContainsNul)
    );
    let mut other = GbaAchievements::new().unwrap();
    for evaluator in [&mut runtime, &mut other] {
        evaluator.activate(1, "0xH008000=18").unwrap();
        assert!(evaluator.evaluate_frame(memory(0)).unwrap().is_empty());
        assert_eq!(evaluator.evaluate_frame(memory(18)).unwrap(), [1]);
    }
}

#[test]
fn multi_byte_read_crosses_the_logical_ram_boundary() {
    let mut runtime = GbaAchievements::new().unwrap();
    runtime.activate(1, "0x007fff=4660").unwrap();
    assert!(runtime
        .evaluate_frame(|_, output| {
            output.fill(0);
            output.len()
        })
        .unwrap()
        .is_empty());
    assert_eq!(
        runtime
            .evaluate_frame(|address, output| {
                assert_eq!(output.len(), 1);
                output[0] = match address {
                    0x03007fff => 0x34,
                    0x02000000 => 0x12,
                    _ => panic!("unexpected native address {address:x}"),
                };
                1
            })
            .unwrap(),
        [1]
    );
}

#[test]
fn service_warnings_never_emit_unlocks() {
    let mut runtime = GbaAchievements::new().unwrap();
    for id in [101_000_001, 101_000_002] {
        runtime.activate(id, "1=1.2.").unwrap();
    }
    runtime.activate(42, "0xH008000=18").unwrap();
    assert!(runtime.evaluate_frame(memory(0)).unwrap().is_empty());
    assert_eq!(runtime.evaluate_frame(memory(18)).unwrap(), [42]);
    for _ in 0..310 {
        assert!(runtime.evaluate_frame(memory(18)).unwrap().is_empty());
    }
}

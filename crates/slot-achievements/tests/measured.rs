use slot_achievements::GbaAchievements;

fn frame(runtime: &mut GbaAchievements, value: u8) -> Vec<u32> {
    runtime
        .evaluate_frame(|_, output| {
            output.fill(value);
            output.len()
        })
        .unwrap()
}

#[test]
fn reports_native_counters_and_percentages_without_evaluating_extra_frames() {
    let mut runtime = GbaAchievements::new().unwrap();
    runtime.activate(1, "M:0xH008000=50").unwrap();
    runtime.activate(2, "G:0xH008000=50").unwrap();
    runtime.activate(3, "0xH008000=50").unwrap();
    frame(&mut runtime, 0);
    frame(&mut runtime, 12);
    assert_eq!(
        runtime.measured_progress(),
        [(1, "12/50".into()), (2, "24%".into())]
    );
    assert_eq!(
        runtime.measured_progress(),
        [(1, "12/50".into()), (2, "24%".into())]
    );
    assert_eq!(frame(&mut runtime, 50), [1, 2, 3]);
    assert!(runtime.measured_progress().is_empty());
}

#[test]
fn resets_and_restored_unlocks_do_not_leave_stale_counters() {
    let mut runtime = GbaAchievements::new().unwrap();
    runtime.activate(1, "M:0xH008000=50").unwrap();
    frame(&mut runtime, 0);
    frame(&mut runtime, 12);
    runtime.reset_progress();
    frame(&mut runtime, 0);
    assert_eq!(runtime.measured_progress(), [(1, "0/50".into())]);
    runtime.restore_unlock(1);
    assert!(runtime.measured_progress().is_empty());
}

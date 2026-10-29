mod common;

use slot::app::Phase;
use slot::audio::StubSink;
use slot::session::Session;
use slot_input::{Btn, Context, RawEvent};

fn tap(session: &mut Session, button: Btn, now: &mut u64) {
    *now += 20;
    session.feed([RawEvent::Down(button)], *now);
    *now += 20;
    session.feed([RawEvent::Up(button)], *now);
    session.update(0.016);
}

#[test]
fn select_on_a_highlighted_rom_shows_saved_progress_without_launching_the_game() {
    let directory = common::tmp_root_with_carts(&["Emerald", "Fusion"]);
    common::clocked(directory.path());
    let rom = directory.path().join("Games/GBA/Emerald.gba");
    let hash = slot_achievements::pack::hash_gba_rom(&rom).unwrap();
    let packs = directory.path().join("Achievements/GBA");
    std::fs::create_dir_all(&packs).unwrap();
    std::fs::write(packs.join(format!("{hash}.json")), format!(
        "{{\"version\":1,\"console_id\":5,\"game_id\":1,\"title\":\"Emerald test pack\",\"rom_hash\":\"{hash}\",\"achievements\":[{{\"id\":42,\"title\":\"Test achievement\",\"description\":\"Saved progress\",\"points\":5,\"rule\":\"0xH008000=3\"}}]}}"
    )).unwrap();
    let mut history = slot_achievements::progress::ProgressStore::open(
        &directory.path().join("Achievements/Progress/GBA"),
        1,
        &hash,
    )
    .unwrap();
    history.record(&[42], 123).unwrap();
    let mut session = Session::boot_with(directory.path().into(), Box::new(StubSink::new()));
    assert!(matches!(session.app().phase(), Phase::Shelf));
    assert_eq!(session.app().selected_stem(), Some("Emerald"));
    let mut now = 0;
    tap(&mut session, Btn::Select, &mut now);
    let view = session
        .app()
        .achievement_view()
        .expect("Select did not open browser");
    assert_eq!(view.game, "Emerald test pack");
    assert_eq!(view.earned(), 1);
    assert_eq!(view.rows[0].status, "Unlocked");
    assert_eq!(session.app().input_context(), Context::Menu);
    assert!(
        !session.has_core(),
        "viewing achievements started an emulator"
    );
    tap(&mut session, Btn::A, &mut now);
    assert!(session.app().achievement_view().unwrap().details);
    tap(&mut session, Btn::B, &mut now);
    assert!(!session.app().achievement_view().unwrap().details);
    tap(&mut session, Btn::B, &mut now);
    assert!(session.app().achievement_view().is_none());
    assert!(matches!(session.app().phase(), Phase::Shelf));
    assert_eq!(session.app().selected_stem(), Some("Emerald"));
    assert_eq!(session.app().input_context(), Context::Shelf);
    assert!(!session.has_core());
}

#[test]
fn missing_pack_shows_an_empty_browser_and_menu_returns_to_the_shelf() {
    let directory = common::tmp_root_with_carts(&["Emerald", "Fusion"]);
    common::clocked(directory.path());
    let mut session = Session::boot_with(directory.path().into(), Box::new(StubSink::new()));
    let mut now = 0;
    tap(&mut session, Btn::Select, &mut now);
    let view = session.app().achievement_view().unwrap();
    assert!(view.rows.is_empty());
    assert!(view.message.contains("No downloaded"));
    tap(&mut session, Btn::Menu, &mut now);
    // Menu taps are dispatched after the double-tap window.
    now += 400;
    session.feed([], now);
    session.update(0.016);
    assert!(session.app().achievement_view().is_none());
    assert!(matches!(session.app().phase(), Phase::Shelf));
    assert!(!session.has_core());
}

#[test]
fn unconfigured_download_points_to_settings_without_inline_keyboard_or_emulation() {
    let directory = common::tmp_root_with_carts(&["Emerald", "Fusion"]);
    common::clocked(directory.path());
    let mut session = Session::boot_with(directory.path().into(), Box::new(StubSink::new()));
    let mut now = 0;
    tap(&mut session, Btn::Select, &mut now);
    assert!(session.app().achievement_view().unwrap().can_download);
    tap(&mut session, Btn::Y, &mut now);
    assert_eq!(session.app().achievement_download().unwrap().step, 4);
    assert!(!session.app().achievement_download().unwrap().can_submit());
    tap(&mut session, Btn::A, &mut now);
    assert_eq!(session.app().achievement_download().unwrap().step, 4);
    tap(&mut session, Btn::B, &mut now);
    assert!(session.app().achievement_download().is_none());
    assert!(session.app().achievement_view().is_some());
    assert!(!session.has_core());
    assert!(!directory
        .path()
        .join("Config/achievements-download.json")
        .exists());
}

fn open_setting(session: &mut Session, row: slot_ui::QuickRow) {
    use slot_input::Action;
    session.app_mut().apply(Action::QuickMenu);
    let mut path = vec![row];
    while let Some(parent) = path.last().and_then(|row| row.parent()) {
        path.push(parent);
    }
    for (depth, step) in path.iter().rev().enumerate() {
        if depth > 0 {
            session.app_mut().apply(Action::GbaDown(Btn::A));
        }
        for _ in 0..step.position(&session.app().hidden_rows()) {
            session.app_mut().apply(Action::GbaDown(Btn::Down));
        }
    }
    assert_eq!(session.app().quick_menu(), Some(row));
    session.app_mut().apply(Action::GbaDown(Btn::A));
    session.update(0.016);
}

#[test]
fn wifi_settings_save_offline_and_return_to_general() {
    let directory = common::tmp_root_with_carts(&["Emerald", "Fusion"]);
    common::clocked(directory.path());
    let mut session = Session::boot_with(directory.path().into(), Box::new(StubSink::new()));
    open_setting(&mut session, slot_ui::QuickRow::Wifi);
    let mut view = session.app().achievement_download().unwrap().clone();
    assert!(view.mode == slot_ui::DownloadMode::Wifi);
    view.setup.ssid = "Home Wi-Fi".into();
    view.setup.wifi_password = "password".into();
    session.app_mut().set_achievement_download(Some(view));
    let mut now = 0;
    tap(&mut session, Btn::Start, &mut now); // password
    tap(&mut session, Btn::Start, &mut now); // review
    tap(&mut session, Btn::A, &mut now); // save, no connection
    assert!(session.app().achievement_download().is_none());
    assert_eq!(session.app().quick_menu(), Some(slot_ui::QuickRow::Wifi));
    let saved = std::fs::read_to_string(directory.path().join("Config/wifi.json")).unwrap();
    assert!(saved.contains("Home Wi-Fi"));
    assert!(!directory
        .path()
        .join("Config/retroachievements.json")
        .exists());
    assert!(!session.has_core());
}

#[test]
fn achievement_settings_open_login_and_batch_entries() {
    for (row, mode, step) in [
        (
            slot_ui::QuickRow::RetroAchievementsLogin,
            slot_ui::DownloadMode::Login,
            2,
        ),
        (
            slot_ui::QuickRow::DownloadAllAchievements,
            slot_ui::DownloadMode::All,
            4,
        ),
    ] {
        let directory = common::tmp_root_with_carts(&["Emerald", "Fusion"]);
        common::clocked(directory.path());
        let mut session = Session::boot_with(directory.path().into(), Box::new(StubSink::new()));
        open_setting(&mut session, row);
        let view = session.app().achievement_download().unwrap();
        assert!(view.mode == mode);
        assert_eq!(view.step, step);
        assert_eq!(view.total_games, 2);
        session.app_mut().apply(slot_input::Action::GbaDown(Btn::B));
        assert!(session.app().achievement_download().is_none());
        assert_eq!(session.app().quick_menu(), Some(row));
    }
}

#[test]
fn local_mode_hides_sync_and_toggle_persists_without_changing_progress() {
    use slot_achievements::mode::{self, Mode};
    use slot_ui::{QuickRow, QuickValue};
    let directory = common::tmp_root_with_carts(&["Emerald", "Fusion"]);
    common::clocked(directory.path());
    let mut session = Session::boot_with(directory.path().into(), Box::new(StubSink::new()));
    assert!(!session.app().sync_enabled());
    assert!(session
        .app()
        .hidden_rows()
        .contains(&QuickRow::SyncProgress));
    assert_eq!(
        session.app().quick_value(QuickRow::AchievementMode),
        Some(QuickValue::Local)
    );
    open_setting(&mut session, QuickRow::AchievementMode);
    assert!(session.app().sync_enabled());
    assert!(!session
        .app()
        .hidden_rows()
        .contains(&QuickRow::SyncProgress));
    assert_eq!(mode::read(directory.path()).unwrap(), Mode::SyncEnabled);
    session
        .app_mut()
        .apply(slot_input::Action::GbaDown(Btn::Left));
    assert!(!session.app().sync_enabled());
    assert!(session
        .app()
        .hidden_rows()
        .contains(&QuickRow::SyncProgress));
    assert_eq!(mode::read(directory.path()).unwrap(), Mode::Local);
}

#[test]
fn individual_game_sync_is_available_only_when_enabled_and_rows_have_plain_status() {
    use slot_achievements::mode::{self, Mode};
    for version in [Mode::Local, Mode::SyncEnabled] {
        let root = common::tmp_root_with_carts(&["Emerald", "Fusion"]);
        common::clocked(root.path());
        mode::write(root.path(), version).unwrap();
        let rom = root.path().join("Games/GBA/Emerald.gba");
        let hash = slot_achievements::pack::hash_gba_rom(&rom).unwrap();
        let directory = root.path().join("Achievements/GBA");
        std::fs::create_dir_all(&directory).unwrap();
        let pack = serde_json::json!({"version":1,"console_id":5,"game_id":1,"title":"Test Game","rom_hash":hash,
            "achievements":[{"id":42,"title":"Test","description":"Test","points":5,"rule":"0xH008000=3"}]});
        std::fs::write(
            directory.join(format!("{hash}.json")),
            serde_json::to_vec(&pack).unwrap(),
        )
        .unwrap();
        let mut history = slot_achievements::progress::ProgressStore::open(
            &root.path().join("Achievements/Progress/GBA"),
            1,
            &hash,
        )
        .unwrap();
        history.record(&[42], 123).unwrap();
        let mut session = Session::boot_with(root.path().into(), Box::new(StubSink::new()));
        let mut now = 0;
        tap(&mut session, Btn::Select, &mut now);
        let view = session.app().achievement_view().unwrap();
        assert_eq!(view.rows[0].status, "Unlocked");
        assert_eq!(view.can_sync, version == Mode::SyncEnabled);
        tap(&mut session, Btn::X, &mut now);
        if version == Mode::Local {
            assert!(session.app().achievement_download().is_none());
        } else {
            assert!(
                session.app().achievement_download().unwrap().mode
                    == slot_ui::DownloadMode::SyncGame
            );
        }
    }
}

#[test]
fn achievements_can_be_disabled_without_losing_mode_or_progress() {
    use slot_achievements::mode::{self, Mode};
    use slot_ui::{QuickRow, QuickValue};
    let directory = common::tmp_root_with_carts(&["Emerald", "Fusion"]);
    common::clocked(directory.path());
    mode::write(directory.path(), Mode::SyncEnabled).unwrap();
    let mut session = Session::boot_with(directory.path().into(), Box::new(StubSink::new()));
    open_setting(&mut session, QuickRow::AchievementEnabled);
    assert!(!session.app().achievements_enabled());
    assert_eq!(
        session.app().quick_value(QuickRow::AchievementEnabled),
        Some(QuickValue::Off)
    );
    assert_eq!(
        QuickRow::AchievementEnabled.shown(&session.app().hidden_rows()),
        [QuickRow::AchievementEnabled]
    );
    assert!(session
        .app()
        .in_game_hidden()
        .contains(&QuickRow::Achievements));
    assert!(!mode::enabled(directory.path()).unwrap());
    assert_eq!(mode::read(directory.path()).unwrap(), Mode::SyncEnabled);
    session.app_mut().apply(slot_input::Action::GbaDown(Btn::B));
    session.app_mut().apply(slot_input::Action::GbaDown(Btn::B));
    session
        .app_mut()
        .apply(slot_input::Action::GbaDown(Btn::Select));
    session.update(0.016);
    assert!(session.app().achievement_view().is_none());
    let mut rebooted = Session::boot_with(directory.path().into(), Box::new(StubSink::new()));
    assert!(!rebooted.app().achievements_enabled());
    open_setting(&mut rebooted, QuickRow::AchievementEnabled);
    assert!(rebooted.app().sync_enabled());
    assert!(mode::enabled(directory.path()).unwrap());
}

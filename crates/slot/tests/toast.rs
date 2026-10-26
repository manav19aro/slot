mod common;

use common::{app_playing_in, tmp_root_with_carts};
use slot_input::Action;
use slot_ui::{Toast, HUD_MS};

#[test]
fn a_save_hotkey_says_state_saved() {
    let d = tmp_root_with_carts(&["Emerald"]);
    let mut a = app_playing_in(d.path(), "Emerald");
    a.apply(Action::SaveState);
    assert_eq!(a.toast(), Some(Toast::StateSaved));
}

#[test]
fn a_load_says_state_loaded() {
    let d = tmp_root_with_carts(&["Emerald"]);
    let mut a = app_playing_in(d.path(), "Emerald");
    a.apply(Action::SaveState);
    a.apply(Action::LoadState);
    assert_eq!(a.toast(), Some(Toast::StateLoaded));
}

#[test]
fn a_refused_load_says_nothing() {
    let d = tmp_root_with_carts(&["Emerald"]);
    let mut a = app_playing_in(d.path(), "Emerald");
    a.apply(Action::LoadState);
    assert_eq!(a.toast(), None, "it announced a load with an empty ring");
    assert!(a.refusal_active(a.now()), "and it did not shake either");
}

#[test]
fn a_toast_goes_away_on_its_own() {
    let d = tmp_root_with_carts(&["Emerald"]);
    let mut a = app_playing_in(d.path(), "Emerald");
    a.apply_at(Action::SaveState, 1_000);
    assert_eq!(a.toast(), Some(Toast::StateSaved));
    a.tick_ms(1_000 + HUD_MS);
    assert_eq!(a.toast(), None);
}

#[test]
fn achievement_notifications_show_and_expire() {
    let d = tmp_root_with_carts(&["Emerald"]);
    let mut app = app_playing_in(d.path(), "Emerald");
    app.achievement_notification(false);
    assert_eq!(app.toast(), Some(Toast::AchievementUnlocked));
    app.tick_ms(app.now() + Toast::AchievementUnlocked.hold_ms());
    assert_eq!(app.toast(), None);
    app.achievement_notification(true);
    assert_eq!(app.toast(), Some(Toast::AchievementsStopped));
}

#[test]
fn unlock_notices_keep_the_name_and_replace_it_for_each_award() {
    let d = tmp_root_with_carts(&["Emerald"]);
    let mut app = app_playing_in(d.path(), "Emerald");
    for title in ["First achievement", "Second achievement"] {
        app.achievement_unlocked(slot_ui::AchievementNotice {
            title: title.into(),
            badge: None,
        });
        assert_eq!(app.achievement_notice().unwrap().title, title);
        app.tick_ms(app.now() + Toast::AchievementUnlocked.hold_ms());
        assert!(app.achievement_notice().is_none());
    }
    app.achievement_notification(true);
    assert!(app.achievement_notice().is_none());
}

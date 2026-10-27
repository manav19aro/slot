mod common;

use std::path::Path;

use common::{tmp_root_with_carts, write_gb_cart};
use slot::app::{App, Phase};
use slot_input::{Action, Btn};
use slot_store::{read_slot_state, write_slot_state, Platform, Recent, SlotState};
use tempfile::TempDir;

fn card() -> TempDir {
    let d = tmp_root_with_carts(&["Emerald", "Fusion"]);
    write_gb_cart(&d, "Tetris", "TETRIS");
    d
}

fn favorite(platform: Platform, stem: &str) -> Recent {
    Recent {
        platform,
        stem: stem.into(),
    }
}

fn boot_with(root: &Path, state: SlotState) -> App {
    write_slot_state(
        root,
        &SlotState {
            clock_set: true,
            ..state
        },
    )
    .expect("write slot.state");
    App::boot(root)
}

fn press(a: &mut App, btn: Btn) {
    common::through(a, btn);
}

fn go_to(a: &mut App, shelf: &str) {
    for _ in 0..8 {
        if a.shelf_platform_name() == Some(shelf) {
            return;
        }
        press(a, Btn::R1);
    }
    panic!("never reached the {shelf} shelf");
}

fn names(a: &mut App) -> Vec<&'static str> {
    let mut names = Vec::new();
    for _ in 0..4 {
        press(a, Btn::R1);
        names.extend(a.shelf_platform_name());
    }
    names
}

#[test]
fn y_on_the_shelf_stars_the_cart_and_saves_it_without_a_banner() {
    let d = card();
    let mut a = boot_with(d.path(), SlotState::default());
    assert!(matches!(a.phase(), Phase::Shelf), "{:?}", a.phase());
    let stem = a.selected_stem().unwrap().to_string();
    press(&mut a, Btn::Y);
    assert_eq!(
        read_slot_state(d.path()).favorites,
        [favorite(Platform::Gba, &stem)]
    );
    assert_eq!(a.toast(), None);
    assert_eq!(
        a.selected_stem(),
        Some(stem.as_str()),
        "starring moved the shelf"
    );
}

#[test]
fn starring_games_never_adds_a_favorites_library_to_navigation() {
    let d = card();
    let mut app = boot_with(d.path(), SlotState::default());
    press(&mut app, Btn::Y);
    assert!(!names(&mut app).contains(&"Favorites"));
    go_to(&mut app, "Game Boy");
    press(&mut app, Btn::Y);
    assert!(!names(&mut app).contains(&"Favorites"));
}

#[test]
fn a_deleted_favorite_is_not_added_to_the_filtered_library() {
    let d = card();
    let mut app = boot_with(d.path(), SlotState {
        favorites: vec![favorite(Platform::Gba,"Deleted"),favorite(Platform::Gba,"Fusion")],
        ..SlotState::default()
    });
    go_to(&mut app,"Game Boy Advance");
    press(&mut app, Btn::X);
    assert_eq!(app.selected_stem(),Some("Fusion"));
}

#[test]
fn x_filters_favorites_and_keeps_the_filter_across_libraries() {
    let d = card();
    let mut app = boot_with(d.path(), SlotState {
        favorites: vec![favorite(Platform::Gba, "Fusion"), favorite(Platform::Gb, "Tetris")],
        ..SlotState::default()
    });
    go_to(&mut app, "Game Boy Advance");
    press(&mut app, Btn::X);
    assert!(app.favorites_only());
    assert_eq!(app.selected_stem(),Some("Fusion"));
    assert_eq!(app.carts().count(),3,"filter must preserve the full catalog");
    assert_eq!(app.slot_text().as_deref(),Some("Game Boy Advance · Favorites"));
    go_to(&mut app,"Game Boy");
    assert!(app.favorites_only());
    assert_eq!(app.selected_stem(),Some("Tetris"));
    go_to(&mut app,"Game Boy Advance");
    assert_eq!(app.selected_stem(),Some("Fusion"));
    press(&mut app, Btn::X);
    assert!(!app.favorites_only());
    assert_eq!(app.selected_stem(),Some("Fusion"),"keep the current game when restoring the library");
    app.apply(Action::ShelfLeft);
    app.apply(Action::GbaUp(Btn::Left));
    for _ in 0..60 { app.update(1.0/60.0); }
    assert_eq!(app.selected_stem(),Some("Emerald"));
}

#[test]
fn libraries_without_favorites_are_skipped_until_the_filter_is_disabled() {
    let d = card();
    let mut app = boot_with(d.path(), SlotState {
        favorites: vec![favorite(Platform::Gba,"Fusion")],
        ..SlotState::default()
    });
    go_to(&mut app,"Game Boy Advance");
    press(&mut app,Btn::X);
    for button in [Btn::R1,Btn::L1] {
        press(&mut app,button);
        assert!(app.favorites_only());
        assert_eq!(app.selected_stem(),Some("Fusion"));
        assert_eq!(app.slot_text().as_deref(),Some("Game Boy Advance · Favorites"));
    }
    press(&mut app,Btn::X);
    go_to(&mut app,"Game Boy");
    assert_eq!(app.selected_stem(),Some("Tetris"));
}

#[test]
fn shoulders_do_nothing_on_a_gba_only_card_with_or_without_the_filter() {
    let d = tmp_root_with_carts(&["Emerald","Fusion"]);
    let mut app = boot_with(d.path(),SlotState {
        favorites: vec![favorite(Platform::Gba,"Fusion")],
        ..SlotState::default()
    });
    for filtered in [false,true] {
        if filtered { press(&mut app,Btn::X); }
        let selected = app.selected_stem().unwrap().to_owned();
        let label = app.slot_text();
        for button in [Btn::R1,Btn::L1,Btn::R1] {
            press(&mut app,button);
            assert_eq!(app.selected_stem(),Some(selected.as_str()));
            assert_eq!(app.slot_text(),label);
            assert_eq!(app.favorites_only(),filtered);
        }
    }
}

#[test]
fn removing_a_favorite_updates_the_filtered_library_without_losing_other_games() {
    let d = card();
    let mut app = boot_with(d.path(),SlotState {
        favorites: vec![favorite(Platform::Gba,"Fusion")],
        ..SlotState::default()
    });
    go_to(&mut app,"Game Boy Advance");
    press(&mut app,Btn::X);
    press(&mut app,Btn::Y);
    assert!(app.favorites_only());
    assert!(app.selected_stem().is_none());
    assert_eq!(app.carts().count(),3);
    press(&mut app,Btn::X);
    assert!(app.selected_stem().is_some());
    assert!(read_slot_state(d.path()).favorites.is_empty());
}

#[test]
fn filtering_an_unstarred_library_is_reversible() {
    let d = card();
    let mut app = boot_with(d.path(),SlotState::default());
    press(&mut app,Btn::X);
    assert!(app.favorites_only());
    assert!(app.selected_stem().is_none());
    press(&mut app,Btn::X);
    assert!(!app.favorites_only());
    assert!(app.selected_stem().is_some());
}

#[test]
fn favorites_filter_also_applies_to_recents() {
    let d = card();
    let mut app = boot_with(d.path(), SlotState {
        show_recents: true,
        recents: vec![favorite(Platform::Gba,"Emerald"),favorite(Platform::Gba,"Fusion")],
        favorites: vec![favorite(Platform::Gba,"Fusion")],
        ..SlotState::default()
    });
    go_to(&mut app,"Recents");
    assert_eq!(app.selected_stem(),Some("Emerald"));
    press(&mut app,Btn::X);
    assert_eq!(app.selected_stem(),Some("Fusion"));
    go_to(&mut app,"Game Boy Advance");
    assert!(app.favorites_only());
    assert_eq!(app.selected_stem(),Some("Fusion"));
    go_to(&mut app,"Recents");
    press(&mut app,Btn::X);
    assert!(!app.favorites_only());
    assert_eq!(app.selected_stem(),Some("Emerald"));
}

#[test]
fn shoulders_consistently_skip_empty_libraries_for_every_platform_combination() {
    // Each platform is absent, populated without favorites, or populated with
    // a favorite. Exercise both shoulder directions from every starting library.
    let platforms = Platform::ALL;
    for combination in 1..27usize {
        let root = tmp_root_with_carts(&[]);
        let mut states = [0usize; 3];
        let mut value = combination;
        let mut favorites = Vec::new();
        for (index, platform) in platforms.iter().copied().enumerate() {
            states[index] = value % 3;
            value /= 3;
            if states[index] == 0 { continue; }
            for suffix in ["One", "Two"] {
                let stem = format!("{} {suffix}", platform.dir_name());
                let mut bytes = vec![0; 0x150];
                if platform == Platform::Gbc { bytes[0x143] = 0x80; }
                let path = root.path().join("Games").join(platform.dir_name()).join(format!("{stem}.{}",platform.extensions()[0]));
                std::fs::write(path, bytes).unwrap();
                if states[index] == 2 && suffix == "One" { favorites.push(favorite(platform, &stem)); }
            }
        }
        for start in 0..3 {
            if states[start] == 0 { continue; }
            for filtered in [false,true] {
                let mut app = boot_with(root.path(),SlotState { favorites:favorites.clone(), ..SlotState::default() });
                for _ in 0..3 {
                    if app.achievement_cart().unwrap().platform == platforms[start] { break; }
                    press(&mut app,Btn::R1);
                }
                assert_eq!(app.achievement_cart().unwrap().platform,platforms[start]);
                if filtered { press(&mut app,Btn::X); }
                let mut current = start;
                for (button, direction) in [(Btn::R1,1i32),(Btn::L1,-1)] {
                    for _ in 0..4 {
                        let expected = (1..=3).map(|step| (current as i32 + direction * step).rem_euclid(3) as usize)
                            .find(|index| if filtered { states[*index] == 2 } else { states[*index] != 0 })
                            .unwrap_or(current);
                        press(&mut app,button);
                        assert_eq!(app.favorites_only(),filtered);
                        let visible = !filtered || states[expected] == 2;
                        if visible {
                            assert_eq!(app.achievement_cart().unwrap().platform,platforms[expected],"combination {combination}, filtered {filtered}, button {button:?}");
                            assert!(app.selected_stem().is_some());
                        } else {
                            assert!(app.selected_stem().is_none());
                            assert_eq!(app.slot_text().as_deref(),Some("No favorites"));
                        }
                        current = expected;
                    }
                }
            }
        }
    }
}

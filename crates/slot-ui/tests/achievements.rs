use slot_ui::{achievement_face, AchievementRow, AchievementView};

fn view() -> AchievementView {
    AchievementView {
        game: "Super Mario World: Super Mario Advance 2".into(),
        rows: (0..7)
            .map(|i| AchievementRow {
                id: 0,
                progress: None,
                badge: None,
                title: if i == 0 {
                    "My Friend, the Dinosaur".into()
                } else {
                    format!("Achievement {}", i + 1)
                },
                description: "Mount a Green Yoshi".into(),
                points: 2,
                status: if i == 0 { "Unlocked" } else { "Locked" },
                earned: i == 0,
            })
            .collect(),
        selected: 0,
        details: false,
        message: String::new(),
        can_download: false,
        can_sync: false,
    }
}

#[test]
fn browsing_clamps_and_counts_only_earned_rows() {
    let mut view = view();
    assert_eq!(view.earned(), 1);
    view.navigate(-1);
    assert_eq!(view.selected, 0);
    view.navigate(5);
    assert_eq!(view.selected, 5);
    view.navigate(100);
    assert_eq!(view.selected, 6);
    view.navigate(-100);
    assert_eq!(view.selected, 0);
    let mut empty = AchievementView::message("", "No local pack");
    empty.navigate(5);
    assert_eq!(empty.selected, 0);
}

#[test]
fn list_and_empty_screens_render_as_opaque_handheld_sized_faces() {
    for view in [
        view(),
        AchievementView::message(
            "Achievements",
            "No downloaded achievement pack for this game",
        ),
    ] {
        let face = achievement_face(&view);
        assert_eq!((face.w, face.h), (640, 480));
        assert!(face.rgba.chunks_exact(4).all(|p| p[3] == 255));
        assert!(face.rgba.chunks_exact(4).any(|p| p[0] > 200));
    }
}

#[test]
fn earned_text_is_green_and_badges_are_small_and_subdued_when_locked() {
    let mut view = view();
    view.rows[0].badge = Some(slot_ui::BadgeImage {
        rgba: [220, 40, 30, 255].repeat(32 * 32),
    });
    let face = achievement_face(&view);
    let pixel = |x: u32, y: u32| &face.rgba[((y * face.w + x) * 4) as usize..][..4];
    assert_eq!(pixel(60, 125), [220, 40, 30, 255]);
    assert!((120..150).any(|y| (100..470).any(|x| pixel(x, y)[1] > pixel(x, y)[0] + 20)));
    view.rows[0].earned = false;
    let locked = achievement_face(&view);
    let at = ((125 * locked.w + 60) * 4) as usize;
    assert_eq!(locked.rgba[at], locked.rgba[at + 1]);
    assert!(locked.rgba[at] < 100);
}

#[test]
fn named_unlock_banner_fits_the_existing_toast_box_with_or_without_a_badge() {
    use slot_ui::{achievement_toast_face, toast_box, AchievementNotice, BadgeImage, Toast};
    let first = achievement_toast_face(&AchievementNotice {
        title: "My Friend, the Dinosaur".into(),
        badge: None,
    });
    let second = achievement_toast_face(&AchievementNotice { title: "A very long achievement name that should be fitted inside this compact notification rather than overflowing onto gameplay".into(), badge: Some(BadgeImage { rgba: [220, 40, 30, 255].repeat(32 * 32) }) });
    assert_eq!((first.w, first.h), toast_box(Toast::AchievementUnlocked));
    assert_eq!((second.w, second.h), (first.w, first.h));
    assert!(first.rgba.chunks_exact(4).any(|p| p[3] != 0));
    assert_ne!(first.rgba, second.rgba);
    assert!(second.h <= 80);
    assert!(second.w <= 514);
    assert_eq!(
        second
            .rgba
            .chunks_exact(4)
            .filter(|p| *p == [220, 40, 30, 255])
            .count(),
        64 * 64,
        "badge should fill its available square without losing its margins"
    );
    assert!(
        second
            .rgba
            .chunks_exact(4)
            .enumerate()
            .filter(|(_, p)| p[3] > 0 && p[0] > 230)
            .all(|(i, _)| i as u32 % second.w >= 88),
        "both heading and achievement name must stay beside the badge"
    );
}

#[test]
fn measured_values_appear_only_in_locked_achievement_details() {
    let mut view = view();
    view.selected = 1;
    let list = achievement_face(&view);
    view.rows[1].progress = Some("12/50".into());
    assert_eq!(list.rgba, achievement_face(&view).rgba);
    view.details = true;
    let measured = achievement_face(&view);
    view.rows[1].progress = None;
    assert_ne!(measured.rgba, achievement_face(&view).rgba);
    view.rows[1].earned = true;
    let earned = achievement_face(&view);
    view.rows[1].progress = Some("12/50".into());
    assert_eq!(earned.rgba, achievement_face(&view).rgba);
}

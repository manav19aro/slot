use slot_ui::{achievement_face, AchievementRow, AchievementView};

fn main() {
    let cache = std::env::args().nth(2);
    let badges = ["412111", "412117", "412118", "412119", "412120"];
    let mut view = AchievementView::message("Super Mario World: Super Mario Advance 2", "");
    for (title, description, points) in [
        ("My Friend, the Dinosaur", "Mount a Green Yoshi", 2),
        (
            "In Search of Dragon Coins",
            "Clear a level after collecting all 5 Dragon Coins",
            5,
        ),
        ("BURN EVERYTHING!", "Obtain a Fire Flower", 2),
        ("Floaty", "Obtain a Cape Feather", 2),
        (
            "Yoshi's Island Defeated",
            "Defeat Iggy Koopa on Yoshi's Island",
            5,
        ),
    ] {
        let earned = view.rows.is_empty();
        view.rows.push(AchievementRow {
            id: 0,
            progress: None,
            badge: cache.as_ref().and_then(|cache| {
                let (pixels, w, h) = slot_ui::decode_png(
                    &std::path::Path::new(cache).join(format!("{}.png", badges[view.rows.len()])),
                )?;
                let mut rgba = vec![0; 64 * 64 * 4];
                for y in 0..64u32 {
                    for x in 0..64u32 {
                        let source = (((y * h / 64) * w + x * w / 64) * 4) as usize;
                        let target = ((y * 64 + x) * 4) as usize;
                        rgba[target..target + 4].copy_from_slice(&pixels[source..source + 4]);
                    }
                }
                Some(slot_ui::BadgeImage { rgba })
            }),
            title: title.into(),
            description: description.into(),
            points,
            status: if earned { "Unlocked" } else { "Locked" },
            earned,
        });
    }
    if let Some(path) = std::env::args().nth(3) {
        let row = &view.rows[0];
        let face = slot_ui::achievement_toast_face(&slot_ui::AchievementNotice {
            title: std::env::args().nth(4).unwrap_or_else(|| row.title.clone()),
            badge: row.badge.clone(),
        });
        save(&path, &face);
    }
    if let Ok(progress) = std::env::var("SLOT_PREVIEW_PROGRESS") {
        view.selected = 1;
        view.details = true;
        view.rows[1].progress = Some(progress);
    }
    let face = achievement_face(&view);
    let path = std::env::args().nth(1).expect("output PNG path");
    save(&path, &face);
}

fn save(path: &str, face: &slot_ui::CartFace) {
    let mut encoder = png::Encoder::new(std::fs::File::create(path).unwrap(), face.w, face.h);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder
        .write_header()
        .unwrap()
        .write_image_data(&face.rgba)
        .unwrap();
}

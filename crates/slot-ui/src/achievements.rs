//! Compact achievement browser. All content comes from local storage.
use crate::{plate, power_menu::MENU_INK, quick_menu, text, CartFace};

/// A small, decoded badge shared by offline menus and notifications.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BadgeImage {
    pub rgba: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AchievementNotice {
    pub title: String,
    pub badge: Option<BadgeImage>,
}

const EARNED_INK: [u8; 3] = [137, 204, 139];

fn row_ink(row: &AchievementRow, selected: bool) -> [u8; 3] {
    if row.earned {
        EARNED_INK
    } else if selected {
        MENU_INK
    } else {
        quick_menu::DIM_INK
    }
}

/// Badge images occupy a fixed 32px square; missing images use a quiet outline.
pub(crate) fn badge_face(badge: Option<&BadgeImage>, earned: bool) -> plate::UndoFace {
    badge_face_sized(badge, earned, 32)
}

pub(crate) fn badge_face_sized(
    badge: Option<&BadgeImage>,
    earned: bool,
    size: u32,
) -> plate::UndoFace {
    let mut rgba = vec![0; (size * size * 4) as usize];
    if let Some(badge) = badge.filter(|b| [32 * 32 * 4, 64 * 64 * 4].contains(&b.rgba.len())) {
        let source_size = if badge.rgba.len() == 64 * 64 * 4 {
            64
        } else {
            32
        };
        for y in 0..size {
            for x in 0..size {
                let source = (((y * source_size / size) * source_size + x * source_size / size) * 4)
                    as usize;
                let target = ((y * size + x) * 4) as usize;
                rgba[target..target + 4].copy_from_slice(&badge.rgba[source..source + 4]);
            }
        }
        if !earned {
            for pixel in rgba.chunks_exact_mut(4) {
                let grey =
                    ((pixel[0] as u32 * 3 + pixel[1] as u32 * 6 + pixel[2] as u32) / 10) as u8;
                pixel[..3].fill(grey / 2);
            }
        }
    } else {
        let ink = if earned {
            EARNED_INK
        } else {
            quick_menu::DIM_INK
        };
        let edge = size as usize * 5 / 32;
        let far = size as usize - edge - 1;
        for y in edge..=far {
            for x in edge..=far {
                if x == edge || x == far || y == edge || y == far {
                    let at = (y * size as usize + x) * 4;
                    rgba[at..at + 3].copy_from_slice(&ink);
                    rgba[at + 3] = 180;
                }
            }
        }
    }
    plate::UndoFace {
        rgba,
        w: size,
        h: size,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AchievementRow {
    pub id: u32,
    pub progress: Option<String>,
    pub badge: Option<BadgeImage>,
    pub title: String,
    pub description: String,
    pub points: u32,
    pub status: &'static str,
    pub earned: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AchievementView {
    pub game: String,
    pub rows: Vec<AchievementRow>,
    pub selected: usize,
    pub details: bool,
    pub message: String,
    pub can_download: bool,
    pub can_sync: bool,
}

impl AchievementView {
    pub fn message(game: &str, message: &str) -> Self {
        Self {
            game: game.into(),
            rows: Vec::new(),
            selected: 0,
            details: false,
            message: message.into(),
            can_download: false,
            can_sync: false,
        }
    }
    pub fn navigate(&mut self, amount: isize) {
        self.selected = self
            .selected
            .saturating_add_signed(amount)
            .min(self.rows.len().saturating_sub(1));
    }
    pub fn earned(&self) -> usize {
        self.rows.iter().filter(|r| r.earned).count()
    }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn label(
    rgba: &mut [u8],
    text: &str,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    px: f32,
    lines: usize,
    colour: [u8; 3],
) {
    let Some(font) = text::label_font() else {
        return;
    };
    let layout = text::fit_box(font, text, w as f32 - 8.0, h as f32, lines, px, px);
    let mut band = vec![0; (w * h * 4) as usize];
    text::draw_centred(&mut band, w, h, &layout, colour);
    for row in 0..h {
        for col in 0..w {
            let source = ((row * w + col) * 4) as usize;
            let target = (((row + y) * 640 + col + x) * 4) as usize;
            let alpha = band[source + 3] as u32;
            for channel in 0..3 {
                rgba[target + channel] = ((band[source + channel] as u32 * alpha
                    + rgba[target + channel] as u32 * (255 - alpha))
                    / 255) as u8;
            }
        }
    }
}

pub(crate) fn overlay(rgba: &mut [u8], face: &plate::UndoFace, x: u32, y: u32) {
    for row in 0..face.h.min(480 - y) {
        for col in 0..face.w.min(640 - x) {
            let source = ((row * face.w + col) * 4) as usize;
            let target = (((row + y) * 640 + col + x) * 4) as usize;
            let alpha = face.rgba[source + 3] as u32;
            for channel in 0..3 {
                rgba[target + channel] = ((face.rgba[source + channel] as u32 * alpha
                    + rgba[target + channel] as u32 * (255 - alpha))
                    / 255) as u8;
            }
        }
    }
}

pub fn achievement_face(view: &AchievementView) -> CartFace {
    let mut rgba = [25, 28, 31, 255].repeat(640 * 480);
    let ink = MENU_INK;
    label(&mut rgba, "Achievements", 20, 12, 600, 36, 28.0, 1, ink);
    label(
        &mut rgba,
        if view.game == "Achievements" {
            ""
        } else {
            &view.game
        },
        24,
        48,
        592,
        28,
        19.0,
        1,
        quick_menu::DIM_INK,
    );
    if view.rows.is_empty() {
        label(&mut rgba, &view.message, 40, 170, 560, 120, 23.0, 4, ink);
    } else if view.details {
        if let Some(row) = view.rows.get(view.selected) {
            overlay(
                &mut rgba,
                &badge_face(row.badge.as_ref(), row.earned),
                32,
                127,
            );
            let earned_ink = row_ink(row, true);
            label(&mut rgba, &row.title, 76, 110, 540, 66, 26.0, 2, earned_ink);
            label(
                &mut rgba,
                &format!("{} - {} pts", row.status, row.points),
                24,
                181,
                592,
                30,
                19.0,
                1,
                earned_ink,
            );
            let measured = row.progress.as_ref().filter(|_| !row.earned);
            label(
                &mut rgba,
                &row.description,
                32,
                222,
                576,
                if measured.is_some() { 162 } else { 202 },
                22.0,
                if measured.is_some() { 6 } else { 8 },
                earned_ink,
            );
            if let Some(progress) = measured {
                label(
                    &mut rgba,
                    &format!("Progress: {progress}"),
                    32,
                    394,
                    576,
                    30,
                    21.0,
                    1,
                    quick_menu::DIM_INK,
                );
            }
        }
    } else {
        label(
            &mut rgba,
            &format!("{} / {} earned", view.earned(), view.rows.len()),
            20,
            79,
            600,
            26,
            20.0,
            1,
            ink,
        );
        let first = (view.selected / 5) * 5;
        for (index, row) in view.rows.iter().enumerate().skip(first).take(5) {
            let y = 116 + ((index - first) * 38) as u32;
            let selected = index == view.selected;
            if selected {
                overlay(&mut rgba, &quick_menu::quick_pointer_face(), 24, y - 6);
            }
            let colour = row_ink(row, selected);
            overlay(
                &mut rgba,
                &badge_face(row.badge.as_ref(), row.earned),
                52,
                y + 2,
            );
            label(&mut rgba, &row.title, 92, y, 390, 36, 20.0, 1, colour);
            label(
                &mut rgba,
                &format!("{} pts", row.points),
                486,
                y,
                124,
                36,
                18.0,
                1,
                colour,
            );
        }
        if let Some(row) = view.rows.get(view.selected) {
            label(
                &mut rgba,
                row.status,
                24,
                319,
                592,
                26,
                19.0,
                1,
                row_ink(row, true),
            );
            label(
                &mut rgba,
                &row.description,
                32,
                350,
                576,
                68,
                21.0,
                3,
                row_ink(row, true),
            );
            let position = if view.message.is_empty() {
                format!("{} / {}", view.selected + 1, view.rows.len())
            } else {
                view.message.clone()
            };
            label(
                &mut rgba,
                &position,
                24,
                422,
                592,
                22,
                17.0,
                1,
                quick_menu::DIM_INK,
            );
        }
    }
    let mut legend: Vec<(&str, &str)> = if view.rows.is_empty() {
        vec![("B", "Back")]
    } else if view.details {
        vec![("B", "Back"), ("A", "List")]
    } else {
        vec![("B", "Back"), ("A", "Details")]
    };
    if view.can_download {
        legend.push(("Y", "Download"));
    }
    if view.can_sync {
        legend.push(("X", "Sync"));
    }
    let hints: Vec<_> = legend
        .iter()
        .map(|(key, label)| plate::hint_face(key, label))
        .collect();
    let width = hints.iter().map(|face| face.w).sum::<u32>()
        + (hints.len().saturating_sub(1) as u32 * plate::HINT_GAP as u32);
    let mut x = (640 - width) / 2;
    for hint in hints {
        overlay(&mut rgba, &hint, x, 449);
        x += hint.w + plate::HINT_GAP as u32;
    }
    CartFace {
        w: 640,
        h: 480,
        rgba,
    }
}

//! Controller-operated setup for a single explicit download.
use crate::{
    achievements::{label, overlay},
    plate,
    power_menu::MENU_INK,
    quick_menu::DIM_INK,
    CartFace,
};

#[derive(Copy, Clone, Default, PartialEq, Eq)]
pub enum DownloadMode {
    #[default]
    Game,
    Wifi,
    Login,
    All,
    SyncGame,
    SyncAll,
}

// No Debug: this state includes passwords while the user is entering them.
#[derive(Clone, Default, PartialEq, Eq)]
pub struct DownloadSetup {
    pub ssid: String,
    pub wifi_password: String,
    pub username: String,
    pub password: String,
    pub saved_sign_in: bool,
}
#[derive(Clone, PartialEq, Eq)]
pub struct DownloadView {
    pub setup: DownloadSetup,
    pub mode: DownloadMode,
    pub completed: bool,
    pub total_games: usize,
    pub step: usize,
    pub selected: usize,
    pub uppercase: bool,
    pub symbols: bool,
    pub status: Option<String>,
    pub cancel_requested: bool,
    pub error: String,
}
impl DownloadView {
    pub fn new(setup: DownloadSetup) -> Self {
        Self {
            setup,
            mode: DownloadMode::Game,
            completed: false,
            total_games: 0,
            step: 0,
            selected: 0,
            uppercase: false,
            symbols: false,
            status: None,
            cancel_requested: false,
            error: String::new(),
        }
    }
    pub fn settings(mode: DownloadMode, setup: DownloadSetup) -> Self {
        let mut view = Self::new(setup);
        view.mode = mode;
        view.step = match mode {
            DownloadMode::Wifi => 0,
            DownloadMode::Login => 2,
            _ => 4,
        };
        view
    }
    pub fn first_step(&self) -> usize {
        if self.mode == DownloadMode::Login {
            2
        } else {
            0
        }
    }
    pub fn can_submit(&self) -> bool {
        if self.completed {
            return false;
        }
        match self.mode {
            DownloadMode::Wifi => !self.setup.ssid.is_empty(),
            DownloadMode::Login => {
                !self.setup.ssid.is_empty()
                    && !self.setup.username.is_empty()
                    && (!self.setup.password.is_empty() || self.setup.saved_sign_in)
            }
            _ => {
                (!matches!(self.mode, DownloadMode::All | DownloadMode::SyncAll)
                    || self.total_games > 0)
                    && !self.setup.ssid.is_empty()
                    && !self.setup.username.is_empty()
                    && self.setup.saved_sign_in
            }
        }
    }

    pub fn field(&self) -> &str {
        match self.step {
            0 => &self.setup.ssid,
            1 => &self.setup.wifi_password,
            2 => &self.setup.username,
            _ => &self.setup.password,
        }
    }
    fn field_mut(&mut self) -> &mut String {
        match self.step {
            0 => &mut self.setup.ssid,
            1 => &mut self.setup.wifi_password,
            2 => &mut self.setup.username,
            _ => &mut self.setup.password,
        }
    }
    pub fn keys(&self) -> Vec<char> {
        if self.symbols {
            "!\"#$%&'()*+,-./:;<=>?@[\\]^_`{|}~".chars().collect()
        } else if self.uppercase {
            "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789-_.@".chars().collect()
        } else {
            "abcdefghijklmnopqrstuvwxyz0123456789-_.@".chars().collect()
        }
    }
    pub fn navigate(&mut self, delta: isize) {
        self.selected = self
            .selected
            .saturating_add_signed(delta)
            .min(self.keys().len() + 1);
    }
    pub fn type_key(&mut self) {
        if self.step == 2 {
            self.setup.saved_sign_in = false;
        }
        let keys = self.keys();
        let max = match self.step {
            0 => 32,
            1 => 64,
            2 => 64,
            _ => 128,
        };
        if self.selected == keys.len() + 1 {
            self.field_mut().pop();
        } else if self.field().len() < max {
            let ch = if self.selected == keys.len() {
                ' '
            } else {
                keys[self.selected]
            };
            self.field_mut().push(ch);
        }
        self.error.clear();
    }
    pub fn next(&mut self) {
        self.error = match self.step {
            0 if self.setup.ssid.is_empty() => "Enter a Wi-Fi network name",
            1 if !self.setup.wifi_password.is_empty() && self.setup.wifi_password.len() < 8 => {
                "Wi-Fi passwords need at least 8 characters"
            }
            1 if self.setup.wifi_password.len() == 64
                && !self
                    .setup
                    .wifi_password
                    .bytes()
                    .all(|c| c.is_ascii_hexdigit()) =>
            {
                "Wi-Fi passwords must be 8 to 63 characters"
            }
            2 if self.setup.username.trim().is_empty() => "Enter your RetroAchievements username",
            3 if self.setup.password.is_empty() && !self.setup.saved_sign_in => {
                "Enter your RetroAchievements password"
            }
            _ => "",
        }
        .into();
        if self.error.is_empty() {
            self.step = if self.mode == DownloadMode::Wifi && self.step == 1 {
                4
            } else {
                (self.step + 1).min(4)
            };
            self.selected = 0;
        }
    }
}

pub fn achievement_download_face(view: &DownloadView) -> CartFace {
    let mut rgba = [25, 28, 31, 255].repeat(640 * 480);
    let ink = MENU_INK;
    label(
        &mut rgba,
        match view.mode {
            DownloadMode::Wifi => "Wi-Fi",
            DownloadMode::Login => "RetroAchievements Login",
            DownloadMode::All => "Download All Achievements",
            DownloadMode::Game => "Download achievements",
            DownloadMode::SyncGame | DownloadMode::SyncAll => "Sync Progress",
        },
        20,
        12,
        600,
        36,
        28.0,
        1,
        ink,
    );
    if let Some(status) = &view.status {
        label(&mut rgba, status, 40, 180, 560, 100, 24.0, 3, ink);
        hints(&mut rgba, &[("B", "Cancel")]);
    } else if view.step == 4 {
        label(
            &mut rgba,
            if view.completed {
                "Finished"
            } else {
                match view.mode {
                    DownloadMode::Wifi => "Save Wi-Fi settings",
                    DownloadMode::Login => "Sign in",
                    DownloadMode::SyncGame | DownloadMode::SyncAll => "Sync account progress",
                    _ => "Ready to download",
                }
            },
            40,
            90,
            560,
            40,
            26.0,
            1,
            ink,
        );
        label(
            &mut rgba,
            &format!("Wi-Fi: {}", view.setup.ssid),
            40,
            145,
            560,
            40,
            22.0,
            1,
            ink,
        );
        label(
            &mut rgba,
            &if view.mode == DownloadMode::Wifi {
                "Used for manual downloads".into()
            } else if matches!(view.mode, DownloadMode::All | DownloadMode::SyncAll) {
                format!("{} GBA games on this card", view.total_games)
            } else {
                format!("Account: {}", view.setup.username)
            },
            40,
            195,
            560,
            40,
            22.0,
            1,
            ink,
        );
        label(
            &mut rgba,
            if !view.error.is_empty() {
                &view.error
            } else if !view.completed
                && view.setup.ssid.is_empty()
                && view.mode != DownloadMode::Wifi
            {
                "Set Wi-Fi in General settings before signing in or downloading."
            } else if !view.completed
                && matches!(view.mode, DownloadMode::All | DownloadMode::SyncAll)
                && view.total_games == 0
            {
                "No GBA games found on this card."
            } else if !view.completed
                && !view.can_submit()
                && matches!(
                    view.mode,
                    DownloadMode::Game
                        | DownloadMode::All
                        | DownloadMode::SyncGame
                        | DownloadMode::SyncAll
                )
            {
                "Set RetroAchievements Login in Achievements settings before downloading."
            } else {
                match view.mode {
                    DownloadMode::Wifi => "Saves Wi-Fi details on the SD card. Connects only when requested.",
                    DownloadMode::Login => "Signs in using saved Wi-Fi. Saves a sign-in token, not your RetroAchievements password.",
                    DownloadMode::All => "Downloads packs and badges for compatible GBA games. Existing unlocks are preserved.",
                    DownloadMode::Game => "Downloads this game's pack and badges using saved settings.",
                    DownloadMode::SyncGame | DownloadMode::SyncAll => "Uploads local achievements to the signed-in account in softcore mode and imports account progress. Sync only runs when you choose it.",
                }
            },
            60,
            265,
            520,
            100,
            19.0,
            4,
            DIM_INK,
        );
        let action = match view.mode {
            DownloadMode::Wifi => "Save",
            DownloadMode::Login => "Sign in",
            DownloadMode::SyncGame | DownloadMode::SyncAll => "Sync",
            _ => "Download",
        };
        if view.can_submit() {
            hints(&mut rgba, &[("B", "Back"), ("A", action)]);
        } else {
            hints(&mut rgba, &[("B", "Back")]);
        }
    } else {
        let title = [
            "Wi-Fi network name",
            "Wi-Fi password",
            "RetroAchievements username",
            "RetroAchievements password",
        ][view.step];
        label(&mut rgba, title, 20, 62, 600, 32, 23.0, 1, ink);
        let secret = view.step == 1 || view.step == 3;
        let field = if secret {
            "*".repeat(view.field().len())
        } else {
            view.field().to_owned()
        };
        // Use a font with a case indicator, consistent with the OS's uppercase typography.
        literal(
            &mut rgba,
            if field.is_empty() { "_" } else { &field },
            32,
            103,
            576,
            42,
            23.0,
            ink,
        );
        let hint = if !view.error.is_empty() {
            view.error.as_str()
        } else if view.step == 3 && view.setup.saved_sign_in {
            "Leave blank to use saved sign-in"
        } else if view.step == 1 {
            "Leave blank for an open network"
        } else if secret {
            "Password stays hidden"
        } else {
            "Use the exact spelling and case"
        };
        label(&mut rgba, hint, 20, 153, 600, 28, 16.0, 1, DIM_INK);
        label(
            &mut rgba,
            if view.symbols {
                "Symbols"
            } else if view.uppercase {
                "Uppercase"
            } else {
                "Lowercase"
            },
            20,
            186,
            600,
            24,
            16.0,
            1,
            DIM_INK,
        );
        let keys = view.keys();
        for index in 0..keys.len() + 2 {
            let (x, y, width, key) = if index < keys.len() {
                (
                    40 + (index % 10) as u32 * 56,
                    218 + (index / 10) as u32 * 36,
                    56,
                    keys[index].to_string(),
                )
            } else {
                (
                    if index == keys.len() { 80 } else { 340 },
                    365,
                    220,
                    if index == keys.len() {
                        "Space"
                    } else {
                        "Delete"
                    }
                    .into(),
                )
            };
            let colour = if view.selected == index { ink } else { DIM_INK };
            literal(&mut rgba, &key, x, y, width, 32, 22.0, colour);
            if view.selected == index {
                for xx in x + 8..x + width - 8 {
                    let at = (((y + 31) * 640 + xx) * 4) as usize;
                    rgba[at..at + 3].copy_from_slice(&ink);
                }
            }
        }
        hints(
            &mut rgba,
            &[
                ("B", "Back"),
                ("A", "Type"),
                ("X", "Case"),
                ("Y", "Symbols"),
                ("START", "Next"),
            ],
        );
    }
    CartFace {
        w: 640,
        h: 480,
        rgba,
    }
}
fn hints(rgba: &mut [u8], legend: &[(&str, &str)]) {
    let faces: Vec<_> = legend
        .iter()
        .map(|(key, label)| plate::hint_face(key, label))
        .collect();
    let total =
        faces.iter().map(|f| f.w).sum::<u32>() + (faces.len().saturating_sub(1) as u32 * 14);
    let mut x = (640 - total) / 2;
    for face in faces {
        overlay(rgba, &face, x, 437);
        x += face.w + 14;
    }
}

#[allow(clippy::too_many_arguments)]
fn literal(rgba: &mut [u8], value: &str, x: u32, y: u32, w: u32, h: u32, px: f32, ink: [u8; 3]) {
    let Some(font) = crate::text::label_font() else {
        return;
    };
    let layout = crate::text::fit_literal(font, value, w as f32 - 8.0, h as f32, px);
    let mut face = plate::UndoFace {
        w,
        h,
        rgba: vec![0; (w * h * 4) as usize],
    };
    crate::text::draw_centred(&mut face.rgba, w, h, &layout, ink);
    overlay(rgba, &face, x, y);
}

use slot_ui::{achievement_download_face, DownloadMode, DownloadSetup, DownloadView};
fn main() {
    let directory = std::env::args().nth(1).expect("output directory");
    std::fs::create_dir_all(&directory).unwrap();
    let setup = DownloadSetup {
        ssid: "Home Wi-Fi".into(),
        username: "Player".into(),
        saved_sign_in: true,
        ..Default::default()
    };
    for (name, mode, step) in [
        ("wifi", DownloadMode::Wifi, 0),
        ("wifi-save", DownloadMode::Wifi, 4),
        ("login", DownloadMode::Login, 2),
        ("sign-in", DownloadMode::Login, 4),
        ("all", DownloadMode::All, 4),
    ] {
        let mut view = DownloadView::settings(mode, setup.clone());
        view.step = step;
        view.total_games = 12;
        let face = achievement_download_face(&view);
        let file = std::fs::File::create(format!("{directory}/{name}.png")).unwrap();
        let mut encoder = png::Encoder::new(file, face.w, face.h);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        encoder
            .write_header()
            .unwrap()
            .write_image_data(&face.rgba)
            .unwrap();
    }
}

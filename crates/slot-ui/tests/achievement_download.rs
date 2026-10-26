use slot_ui::{achievement_download_face, DownloadSetup, DownloadView};
#[test]
fn keyboard_preserves_case_handles_symbols_spaces_and_delete() {
    let mut view = DownloadView::new(DownloadSetup::default());
    view.type_key();
    view.uppercase = true;
    view.type_key();
    view.selected = view.keys().len();
    view.type_key();
    assert_eq!(view.setup.ssid, "aA ");
    view.selected += 1;
    view.type_key();
    assert_eq!(view.setup.ssid, "aA");
    view.symbols = true;
    view.selected = 0;
    view.type_key();
    assert_eq!(view.setup.ssid, "aA!");
}
#[test]
fn setup_requires_valid_fields_and_can_reuse_a_saved_sign_in() {
    let mut view = DownloadView::new(DownloadSetup::default());
    view.next();
    assert_eq!(view.step, 0);
    assert!(!view.error.is_empty());
    view.setup.ssid = "Home".into();
    view.next();
    view.setup.wifi_password = "short".into();
    view.next();
    assert_eq!(view.step, 1);
    view.setup.wifi_password = "password".into();
    view.next();
    view.setup.username = "Player".into();
    view.next();
    view.next();
    assert_eq!(view.step, 3);
    view.setup.saved_sign_in = true;
    view.next();
    assert_eq!(view.step, 4);
    for step in 0..=4 {
        view.step = step;
        let face = achievement_download_face(&view);
        assert_eq!((face.w, face.h), (640, 480));
        assert!(face.rgba.chunks_exact(4).all(|p| p[3] == 255));
    }
}

#[test]
fn settings_edit_only_their_own_fields_and_configured_download_needs_no_keyboard() {
    use slot_ui::DownloadMode;
    let setup = DownloadSetup {
        ssid: "Home".into(),
        wifi_password: "password".into(),
        username: "Player".into(),
        password: String::new(),
        saved_sign_in: true,
    };
    let mut wifi = DownloadView::settings(DownloadMode::Wifi, setup.clone());
    assert_eq!(wifi.step, 0);
    wifi.next();
    assert_eq!(wifi.step, 1);
    wifi.next();
    assert_eq!(wifi.step, 4);
    assert!(wifi.can_submit());
    let login = DownloadView::settings(DownloadMode::Login, setup.clone());
    assert_eq!(login.step, 2);
    let game = DownloadView::settings(DownloadMode::Game, setup.clone());
    assert_eq!(game.step, 4);
    assert!(game.can_submit());
    let mut all = DownloadView::settings(DownloadMode::All, setup);
    assert_eq!(all.step, 4);
    assert!(!all.can_submit());
    all.total_games = 3;
    assert!(all.can_submit());
}

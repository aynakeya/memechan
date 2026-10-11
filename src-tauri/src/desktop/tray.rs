use tauri::{
    menu::{Menu, MenuItem, PredefinedMenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    App, AppHandle, Manager,
};

pub(super) fn show_main_window(app: &AppHandle) {
    let window = app.get_webview_window("main").expect("主窗口不存在");
    if let Err(error) = window
        .show()
        .and_then(|()| window.unminimize())
        .and_then(|()| window.set_focus())
    {
        eprintln!("显示窗口失败：{error}");
    }
}

pub(super) fn setup(app: &App) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "tray_show", "显示窗口", true, None::<&str>)?;
    let hide = MenuItem::with_id(app, "tray_hide", "隐藏窗口", true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "tray_quit", "退出 Memechan", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &hide, &separator, &quit])?;

    TrayIconBuilder::with_id("main")
        .icon(app.default_window_icon().expect("应用图标未配置").clone())
        .tooltip("Memechan")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "tray_show" => show_main_window(app),
            "tray_hide" => {
                let window = app.get_webview_window("main").expect("主窗口不存在");
                if let Err(error) = window.hide() {
                    eprintln!("隐藏窗口失败：{error}");
                }
            }
            "tray_quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

#![windows_subsystem = "windows"]

#[cfg(windows)]
mod appmsg;
mod config;
mod dock;
#[cfg(windows)]
mod drag;
#[cfg(windows)]
mod frame;
mod layout;
#[cfg(windows)]
mod overlay;
#[cfg(windows)]
mod win_util;

#[cfg(windows)]
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};

/// 非表示のメッセージ専用ウィンドウ。フックイベントとトレイ通知の受け口
#[cfg(windows)]
unsafe extern "system" fn app_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    use windows::Win32::UI::WindowsAndMessaging::DefWindowProcW;

    if drag::handle_app_message(hwnd, msg, wparam) {
        return LRESULT(0);
    }
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

#[cfg(windows)]
fn main() {
    use windows::core::w;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::WindowsAndMessaging::*;

    frame::register_class();
    frame::set_save_hook(save_config);

    unsafe {
        let wc = WNDCLASSW {
            lpfnWndProc: Some(app_wndproc),
            hInstance: GetModuleHandleW(None).unwrap().into(),
            lpszClassName: w!("WndPanelApp"),
            ..Default::default()
        };
        RegisterClassW(&wc);
        let app_hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("WndPanelApp"),
            w!(""),
            WINDOW_STYLE::default(),
            0,
            0,
            0,
            0,
            Some(HWND_MESSAGE),
            None,
            Some(GetModuleHandleW(None).unwrap().into()),
            None,
        )
        .expect("app window creation failed");

        let overlay = overlay::create();
        frame::APP.with(|a| {
            let mut app = a.borrow_mut();
            app.app_hwnd = app_hwnd;
            app.overlay = overlay;
        });
        drag::install_hooks(app_hwnd);
    }

    let cfg = config::load(&config::config_path());
    let frames =
        if cfg.frame.is_empty() { vec![config::FrameConfig::default()] } else { cfg.frame.clone() };
    for fc in &frames {
        frame::create(fc);
    }

    unsafe {
        let mut msg = MSG::default();
        while GetMessageW(&mut msg, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

/// 全フレームの状態を config.toml に書き出す(frame から SAVE_HOOK 経由で呼ばれる)
#[cfg(windows)]
fn save_config() {
    let cfg = frame::APP.with(|a| {
        let app = a.borrow();
        config::Config {
            frame: app
                .frames
                .iter()
                .map(|f| config::FrameConfig {
                    preset: f.preset,
                    x: f.restore_rect.x,
                    y: f.restore_rect.y,
                    width: f.restore_rect.w,
                    height: f.restore_rect.h,
                    maximized: f.maximized,
                })
                .collect(),
        }
    });
    let _ = config::save(&config::config_path(), &cfg);
}

#[cfg(not(windows))]
fn main() {
    eprintln!("This tool runs on Windows only. (ロジックのテストは cargo test で実行可)");
}

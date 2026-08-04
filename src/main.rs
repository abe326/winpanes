#![windows_subsystem = "windows"]

#[cfg(windows)]
mod appmsg;
mod config;
mod dock;
#[cfg(windows)]
mod frame;
mod layout;
#[cfg(windows)]
mod overlay;
#[cfg(windows)]
mod win_util;

#[cfg(windows)]
fn main() {
    use windows::Win32::UI::WindowsAndMessaging::{
        DispatchMessageW, GetMessageW, TranslateMessage, MSG,
    };

    frame::register_class();
    frame::set_save_hook(save_config);

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

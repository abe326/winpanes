#![cfg(windows)]
use crate::appmsg::WM_APP_TRAY;
use windows::core::w;
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::*;

const TRAY_ID: u32 = 1;
pub const CMD_NEW_FRAME: usize = 1001;
pub const CMD_PRESET_BASE: usize = 1100; // +1 grid2x2 / +2 cols2 / +3 rows2
pub const CMD_QUIT: usize = 1900;

fn base_icon_data(app_hwnd: HWND) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW {
        cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
        hWnd: app_hwnd,
        uID: TRAY_ID,
        ..Default::default()
    }
}

pub fn add(app_hwnd: HWND) {
    unsafe {
        let mut nid = base_icon_data(app_hwnd);
        nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
        nid.uCallbackMessage = WM_APP_TRAY;
        nid.hIcon = LoadIconW(None, IDI_APPLICATION).unwrap_or_default();
        let tip: Vec<u16> = "Window Panel Tool\0".encode_utf16().collect();
        nid.szTip[..tip.len()].copy_from_slice(&tip);
        let _ = Shell_NotifyIconW(NIM_ADD, &nid);
    }
}

pub fn remove(app_hwnd: HWND) {
    unsafe {
        let _ = Shell_NotifyIconW(NIM_DELETE, &base_icon_data(app_hwnd));
    }
}

/// 右クリックメニュー。選択結果は WM_COMMAND で app_wndproc に届く
pub fn show_menu(app_hwnd: HWND) {
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        let _ = AppendMenuW(menu, MF_STRING, CMD_NEW_FRAME, w!("新しいフレーム"));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        let _ = AppendMenuW(menu, MF_STRING, CMD_PRESET_BASE + 1, w!("4分割"));
        let _ = AppendMenuW(menu, MF_STRING, CMD_PRESET_BASE + 2, w!("縦2分割"));
        let _ = AppendMenuW(menu, MF_STRING, CMD_PRESET_BASE + 3, w!("横2分割"));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, None);
        let _ = AppendMenuW(menu, MF_STRING, CMD_QUIT, w!("終了"));
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        // TrackPopupMenu の作法: 前面化しないとメニューが閉じなくなる
        let _ = SetForegroundWindow(app_hwnd);
        let _ = TrackPopupMenu(menu, TPM_RIGHTBUTTON, pt.x, pt.y, None, app_hwnd, None);
        let _ = DestroyMenu(menu);
    }
}

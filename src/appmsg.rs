#![cfg(windows)]
use windows::Win32::UI::WindowsAndMessaging::WM_APP;

pub const WM_APP_DRAGSTART: u32 = WM_APP + 1; // wparam = 対象HWND
pub const WM_APP_DRAGEND: u32 = WM_APP + 2; // wparam = 対象HWND
pub const WM_APP_DESTROYED: u32 = WM_APP + 3; // wparam = 対象HWND
pub const WM_APP_TRAY: u32 = WM_APP + 4; // トレイアイコンのコールバック
pub const WM_APP_QUIT: u32 = WM_APP + 5; // 最後のフレームが閉じた → アプリ終了
pub const TIMER_DRAG: usize = 1;

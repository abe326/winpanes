#![cfg(windows)]
use crate::layout::Rect;
use std::cell::RefCell;
use windows::core::w;
use windows::core::BOOL;
use windows::Win32::Foundation::{CloseHandle, COLORREF, HANDLE, HWND, RECT};
use windows::Win32::Graphics::Dwm::{DwmGetColorizationColor, DwmGetWindowAttribute, DWMWA_CLOAKED};
use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER};
use windows::Win32::System::Registry::{RegGetValueW, HKEY_CURRENT_USER, RRF_RT_REG_DWORD};
use windows::Win32::System::Threading::{
    OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION,
};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Shell::{ITaskbarList, TaskbarList};
use windows::Win32::UI::WindowsAndMessaging::*;

thread_local! {
    /// UIスレッド専用の ITaskbarList(初回使用時に生成)
    static TASKBAR: RefCell<Option<ITaskbarList>> = const { RefCell::new(None) };
}

/// ドック中ウィンドウのタスクバーボタンを隠す/戻す。
/// ITaskbarList はボタンの表示だけを制御し、対象ウィンドウのスタイルには
/// 触れないため他アプリへの副作用がない(Alt+Tab には残る)
pub fn set_taskbar_visible(hwnd: HWND, visible: bool) {
    TASKBAR.with(|t| {
        let mut slot = t.borrow_mut();
        if slot.is_none() {
            unsafe {
                if let Ok(list) =
                    CoCreateInstance::<_, ITaskbarList>(&TaskbarList, None, CLSCTX_INPROC_SERVER)
                {
                    if list.HrInit().is_ok() {
                        *slot = Some(list);
                    }
                }
            }
        }
        if let Some(list) = slot.as_ref() {
            unsafe {
                let _ = if visible { list.AddTab(hwnd) } else { list.DeleteTab(hwnd) };
            }
        }
    });
}

pub fn to_rect(r: RECT) -> Rect {
    Rect { x: r.left, y: r.top, w: r.right - r.left, h: r.bottom - r.top }
}

pub fn to_win_rect(r: Rect) -> RECT {
    RECT { left: r.x, top: r.y, right: r.x + r.w, bottom: r.y + r.h }
}

pub fn window_rect(hwnd: HWND) -> Option<Rect> {
    let mut r = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut r).ok().map(|_| to_rect(r)) }
}

/// 仕様8: 失敗したら false を返し、呼び出し側がそのウィンドウだけ解除する
pub fn move_window_to(hwnd: HWND, r: Rect) -> bool {
    unsafe { SetWindowPos(hwnd, None, r.x, r.y, r.w, r.h, SWP_NOZORDER | SWP_NOACTIVATE).is_ok() }
}

pub fn dpi_scale(hwnd: HWND, logical: i32) -> i32 {
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    if dpi == 0 {
        logical
    } else {
        (logical * dpi as i32 + 48) / 96
    }
}

/// 管理対象になり得るウィンドウか(可視・トップレベル・通常ウィンドウ・自プロセス以外)
pub fn is_manageable(hwnd: HWND) -> bool {
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() {
            return false;
        }
        if GetAncestor(hwnd, GA_ROOT) != hwnd {
            return false;
        }
        let ex = GetWindowLongW(hwnd, GWL_EXSTYLE) as u32;
        if ex & WS_EX_TOOLWINDOW.0 != 0 {
            return false;
        }
        // UWP等のクロークされたウィンドウを除外
        let mut cloaked: u32 = 0;
        if DwmGetWindowAttribute(hwnd, DWMWA_CLOAKED, &mut cloaked as *mut _ as *mut _, 4).is_ok()
            && cloaked != 0
        {
            return false;
        }
        // 自プロセスのウィンドウ(フレーム・オーバーレイ)は対象外
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == std::process::id() {
            return false;
        }
        true
    }
}

/// 管理者権限プロセスのウィンドウか(仕様9: 操作不可)。判定不能時は true(=触らない)に倒す
pub fn is_elevated_window(hwnd: HWND) -> bool {
    unsafe {
        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        let Ok(proc) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else {
            return true; // 開けない = 保護されている/権限が上
        };
        let mut token = HANDLE::default();
        let mut elevated = false;
        if OpenProcessToken(proc, TOKEN_QUERY, &mut token).is_ok() {
            let mut info = TOKEN_ELEVATION::default();
            let mut len = 0u32;
            if GetTokenInformation(
                token,
                TokenElevation,
                Some(&mut info as *mut _ as *mut _),
                std::mem::size_of::<TOKEN_ELEVATION>() as u32,
                &mut len,
            )
            .is_ok()
            {
                elevated = info.TokenIsElevated != 0;
            }
            let _ = CloseHandle(token);
        }
        let _ = CloseHandle(proc);
        elevated
    }
}

/// UI配色。COLORREF は 0x00BBGGRR
#[derive(Clone, Copy)]
pub struct Theme {
    pub bg: COLORREF,
    pub toolbar: COLORREF,
    pub header: COLORREF,
    pub text: COLORREF,
    pub text_dim: COLORREF,
    pub border: COLORREF,
    pub accent: COLORREF,
    pub hover: COLORREF,
    pub denied: COLORREF,
}

const fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF((b as u32) << 16 | (g as u32) << 8 | r as u32)
}

/// システムテーマ(ライト/ダーク)とアクセントカラーに追従(仕様6: UIデザイン方針)
pub fn detect_theme() -> Theme {
    let light = unsafe {
        let mut val: u32 = 1;
        let mut size = 4u32;
        let ok = RegGetValueW(
            HKEY_CURRENT_USER,
            w!(r"Software\Microsoft\Windows\CurrentVersion\Themes\Personalize"),
            w!("AppsUseLightTheme"),
            RRF_RT_REG_DWORD,
            None,
            Some(&mut val as *mut _ as *mut _),
            Some(&mut size),
        );
        ok.is_ok() && val != 0
    };
    let accent = unsafe {
        let mut argb: u32 = 0;
        let mut opaque = BOOL::default();
        if DwmGetColorizationColor(&mut argb, &mut opaque).is_ok() {
            // ARGB -> COLORREF(R/B入れ替え)
            rgb(((argb >> 16) & 0xFF) as u8, ((argb >> 8) & 0xFF) as u8, (argb & 0xFF) as u8)
        } else {
            rgb(0x00, 0x78, 0xD4) // Windows既定の青
        }
    };
    if light {
        Theme {
            bg: rgb(0xF3, 0xF3, 0xF3),
            toolbar: rgb(0xEB, 0xEB, 0xEB),
            header: rgb(0xE5, 0xE5, 0xE5),
            text: rgb(0x1A, 0x1A, 0x1A),
            text_dim: rgb(0x60, 0x60, 0x60),
            border: rgb(0xD0, 0xD0, 0xD0),
            accent,
            hover: rgb(0xDA, 0xDA, 0xDA),
            denied: rgb(0xC4, 0x2B, 0x1C),
        }
    } else {
        Theme {
            bg: rgb(0x20, 0x20, 0x20),
            toolbar: rgb(0x2B, 0x2B, 0x2B),
            header: rgb(0x33, 0x33, 0x33),
            text: rgb(0xF0, 0xF0, 0xF0),
            text_dim: rgb(0xA0, 0xA0, 0xA0),
            border: rgb(0x45, 0x45, 0x45),
            accent,
            hover: rgb(0x3D, 0x3D, 0x3D),
            denied: rgb(0xE8, 0x11, 0x23),
        }
    }
}

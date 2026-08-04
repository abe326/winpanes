#![cfg(windows)]
use crate::layout::Rect;
use windows::core::w;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::{CreateSolidBrush, DeleteObject, FillRect, GetDC, ReleaseDC};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;

/// 既定処理のみ。windows クレートの DefWindowProcW は Rust fn なので直接は登録できない
unsafe extern "system" fn overlay_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

/// クリック透過・非アクティブの半透明オーバーレイ(仕様3)
pub fn create() -> HWND {
    unsafe {
        let wc = WNDCLASSW {
            lpfnWndProc: Some(overlay_wndproc),
            hInstance: GetModuleHandleW(None).unwrap().into(),
            lpszClassName: w!("WndPanelOverlay"),
            ..Default::default()
        };
        RegisterClassW(&wc);
        let hwnd = CreateWindowExW(
            WS_EX_LAYERED
                | WS_EX_TRANSPARENT
                | WS_EX_TOPMOST
                | WS_EX_NOACTIVATE
                | WS_EX_TOOLWINDOW,
            w!("WndPanelOverlay"),
            w!(""),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(GetModuleHandleW(None).unwrap().into()),
            None,
        )
        .expect("overlay creation failed");
        // 透明度 ~38%(96/255)
        let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 96, LWA_ALPHA);
        hwnd
    }
}

pub fn show(hwnd: HWND, r: Rect, denied: bool) {
    unsafe {
        let color = crate::frame::APP.with(|a| {
            let t = a.borrow().theme;
            if denied {
                t.denied
            } else {
                t.accent
            }
        });
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOPMOST),
            r.x,
            r.y,
            r.w,
            r.h,
            SWP_NOACTIVATE | SWP_SHOWWINDOW,
        );
        // 単色塗り(WM_PAINT を経由しない直接描画で十分)
        let dc = GetDC(Some(hwnd));
        let brush = CreateSolidBrush(color);
        let rect = RECT { left: 0, top: 0, right: r.w, bottom: r.h };
        FillRect(dc, &rect, brush);
        let _ = DeleteObject(brush.into());
        ReleaseDC(Some(hwnd), dc);
    }
}

pub fn hide(hwnd: HWND) {
    unsafe {
        let _ = ShowWindow(hwnd, SW_HIDE);
    }
}

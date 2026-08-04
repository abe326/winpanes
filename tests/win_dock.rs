//! Windows 実機での smoke テスト。
//! 実行: cargo test --target x86_64-pc-windows-gnu --test win_dock -- --test-threads=1
//! (ウィンドウを生成するため直列実行が必要)
//!
//! ドック台帳のロジックそのものは src/dock.rs の単体テストで担保している。
//! ここで確認するのは「SetWindowPos ベースの物理移動が実環境で機能するか」だけ。
#![cfg(windows)]

use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::*;

unsafe extern "system" fn test_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
}

fn make_test_window(x: i32, y: i32, w_: i32, h: i32) -> HWND {
    unsafe {
        let wc = WNDCLASSW {
            lpfnWndProc: Some(test_wndproc),
            hInstance: GetModuleHandleW(None).unwrap().into(),
            lpszClassName: w!("WptTestWnd"),
            ..Default::default()
        };
        RegisterClassW(&wc); // 2回目以降の登録失敗は想定内
        CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            w!("WptTestWnd"),
            w!("test"),
            WS_OVERLAPPEDWINDOW | WS_VISIBLE,
            x,
            y,
            w_,
            h,
            None,
            None,
            Some(GetModuleHandleW(None).unwrap().into()),
            None,
        )
        .unwrap()
    }
}

fn window_rect(hwnd: HWND) -> (i32, i32, i32, i32) {
    unsafe {
        let mut r = RECT::default();
        GetWindowRect(hwnd, &mut r).unwrap();
        (r.left, r.top, r.right - r.left, r.bottom - r.top)
    }
}

#[test]
fn set_window_pos_moves_real_window() {
    let hwnd = make_test_window(50, 50, 300, 200);
    unsafe {
        SetWindowPos(hwnd, None, 400, 300, 500, 400, SWP_NOZORDER | SWP_NOACTIVATE).unwrap();
        assert_eq!(window_rect(hwnd), (400, 300, 500, 400));
        let _ = DestroyWindow(hwnd);
    }
}

#[test]
fn defer_window_pos_moves_multiple_windows_at_once() {
    // reflow() が使う一括移動が実環境で効くことの確認
    let a = make_test_window(0, 0, 200, 200);
    let b = make_test_window(0, 0, 200, 200);
    unsafe {
        let mut hdwp = BeginDeferWindowPos(2).unwrap();
        hdwp = DeferWindowPos(hdwp, a, None, 100, 100, 300, 300, SWP_NOZORDER | SWP_NOACTIVATE)
            .unwrap();
        hdwp = DeferWindowPos(hdwp, b, None, 500, 100, 300, 300, SWP_NOZORDER | SWP_NOACTIVATE)
            .unwrap();
        EndDeferWindowPos(hdwp).unwrap();
        assert_eq!(window_rect(a), (100, 100, 300, 300));
        assert_eq!(window_rect(b), (500, 100, 300, 300));
        let _ = DestroyWindow(a);
        let _ = DestroyWindow(b);
    }
}

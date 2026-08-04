#![cfg(windows)]
use crate::appmsg::*;
use crate::dock::DockResult;
use crate::frame::{panels_screen, DragCtx, APP};
use crate::layout::Rect;
use crate::win_util::*;
use std::sync::atomic::{AtomicIsize, Ordering};
use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
use windows::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
use windows::Win32::UI::WindowsAndMessaging::*;

static APP_HWND: AtomicIsize = AtomicIsize::new(0);

/// 他プロセスのウィンドウ移動を監視する(プロセス外フック。DLL注入なし)
pub fn install_hooks(app_hwnd: HWND) {
    APP_HWND.store(app_hwnd.0 as isize, Ordering::SeqCst);
    unsafe {
        SetWinEventHook(
            EVENT_SYSTEM_MOVESIZESTART,
            EVENT_SYSTEM_MOVESIZEEND,
            None,
            Some(win_event_proc),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        );
        SetWinEventHook(
            EVENT_OBJECT_DESTROY,
            EVENT_OBJECT_DESTROY,
            None,
            Some(win_event_proc),
            0,
            0,
            WINEVENT_OUTOFCONTEXT | WINEVENT_SKIPOWNPROCESS,
        );
    }
}

/// フックコールバックはメッセージを投げるだけ(処理は UI スレッドで行う)
unsafe extern "system" fn win_event_proc(
    _hook: HWINEVENTHOOK,
    event: u32,
    hwnd: HWND,
    id_object: i32,
    id_child: i32,
    _tid: u32,
    _time: u32,
) {
    // ウィンドウ本体のイベントのみ(子オブジェクトは無視)
    if id_object != OBJID_WINDOW.0 || id_child != 0 {
        return;
    }
    let msg = match event {
        EVENT_SYSTEM_MOVESIZESTART => WM_APP_DRAGSTART,
        EVENT_SYSTEM_MOVESIZEEND => WM_APP_DRAGEND,
        EVENT_OBJECT_DESTROY => WM_APP_DESTROYED,
        _ => return,
    };
    let app = HWND(APP_HWND.load(Ordering::SeqCst) as *mut _);
    unsafe {
        let _ = PostMessageW(Some(app), msg, WPARAM(hwnd.0 as usize), LPARAM(0));
    }
}

/// アプリウィンドウの wndproc から呼ぶ。処理したら true
pub fn handle_app_message(app_hwnd: HWND, msg: u32, wparam: WPARAM) -> bool {
    match msg {
        WM_APP_DRAGSTART => {
            on_drag_start(app_hwnd, HWND(wparam.0 as *mut _));
            true
        }
        WM_APP_DRAGEND => {
            on_drag_end(app_hwnd, HWND(wparam.0 as *mut _));
            true
        }
        WM_APP_DESTROYED => {
            on_destroyed(HWND(wparam.0 as *mut _));
            true
        }
        WM_TIMER if wparam.0 == TIMER_DRAG => {
            on_drag_tick();
            true
        }
        _ => false,
    }
}

fn on_drag_start(app_hwnd: HWND, target: HWND) {
    if !is_manageable(target) {
        return;
    }
    let denied = is_elevated_window(target);
    let id = target.0 as isize;
    APP.with(|a| {
        let mut app = a.borrow_mut();
        // ロック中パネルの占有者か。ドラッグ中はマウスが塞がりロック操作は
        // 起こらないため、開始時の判定で確定してよい
        let from_locked = app
            .frames
            .iter()
            .any(|f| f.dock.panel_of(id).is_some_and(|p| f.dock.is_locked(p)));
        app.dragging = Some(DragCtx { target: id, denied, from_locked });
    });
    // ドラッグ中のみポーリング(平常時のCPU負荷はゼロ)
    unsafe {
        SetTimer(Some(app_hwnd), TIMER_DRAG, 16, None);
    }
}

/// カーソル下のフレーム/パネルを求める。戻り値: (frame index, panel index, panel body rect)
fn hit_panel(cursor: POINT) -> Option<(usize, usize, Rect)> {
    APP.with(|a| {
        let app = a.borrow();
        for (fi, f) in app.frames.iter().enumerate() {
            for (pi, p) in panels_screen(f).iter().enumerate() {
                // ヘッダー・ボディどちらに落としてもそのパネルとみなす
                if p.header.contains(cursor.x, cursor.y) || p.body.contains(cursor.x, cursor.y) {
                    return Some((fi, pi, p.body));
                }
            }
        }
        None
    })
}

fn cursor_pos() -> POINT {
    let mut pt = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut pt);
    }
    pt
}

fn on_drag_tick() {
    let (overlay, flags) = APP.with(|a| {
        let app = a.borrow();
        (app.overlay, app.dragging.as_ref().map(|d| (d.denied, d.from_locked)))
    });
    let Some((denied, from_locked)) = flags else { return };
    match hit_panel(cursor_pos()) {
        Some((fi, pi, body)) => {
            let dest_locked =
                APP.with(|a| a.borrow().frames.get(fi).is_some_and(|f| f.dock.is_locked(pi)));
            crate::overlay::show(overlay, body, denied || from_locked || dest_locked);
        }
        None => crate::overlay::hide(overlay),
    }
}

fn on_drag_end(app_hwnd: HWND, target: HWND) {
    unsafe {
        let _ = KillTimer(Some(app_hwnd), TIMER_DRAG);
    }
    let ctx = APP.with(|a| a.borrow_mut().dragging.take());
    let overlay = APP.with(|a| a.borrow().overlay);
    crate::overlay::hide(overlay);
    let Some(ctx) = ctx else { return };
    if ctx.target != target.0 as isize || ctx.denied {
        return; // 別ウィンドウのイベント、または権限で操作不可
    }

    let id = ctx.target;
    // 完全ロック: ロック中パネルの占有者はどこへドロップしても元のパネルへ戻す
    if ctx.from_locked {
        snap_back(id);
        return;
    }
    match hit_panel(cursor_pos()) {
        Some((fi, pi, _)) => {
            // ロック中パネルへのドロップは無効。ドック済みなら元のパネルへ戻す
            let dest_locked =
                APP.with(|a| a.borrow().frames.get(fi).is_some_and(|f| f.dock.is_locked(pi)));
            if dest_locked {
                snap_back(id);
                return;
            }
            // フレームを跨ぐ移動に備え、いったんどのフレームからも外す
            let prev = undock_from_any_frame(id);
            let orig = prev.unwrap_or_else(|| window_rect(target).unwrap_or_default());
            let mut freed = None;
            let mut frame_hwnd = None;
            APP.with(|a| {
                let mut app = a.borrow_mut();
                if let Some(f) = app.frames.get_mut(fi) {
                    frame_hwnd = Some(f.hwnd);
                    if let DockResult::Replaced { freed: prev_occupant } = f.dock.dock(id, pi, orig)
                    {
                        freed = Some(prev_occupant);
                    }
                }
            });
            // 押し出された先住ウィンドウは元のサイズへ復元(仕様4)
            if let Some(d) = freed {
                move_window_to(HWND(d.id as *mut _), d.orig);
            }
            if let Some(h) = frame_hwnd {
                crate::frame::reflow(h);
            }
        }
        None => {
            // フレーム外で解除: サイズは元に戻し、位置はドロップ地点のまま(仕様4)
            if let Some(orig) = undock_from_any_frame(id) {
                if let Some(cur) = window_rect(target) {
                    move_window_to(target, Rect { x: cur.x, y: cur.y, w: orig.w, h: orig.h });
                }
            }
        }
    }
}

/// ドック済みウィンドウを所属パネルの位置へ戻す(ロック拒否時のスナップバック)。
/// 未ドックなら何もしない(ドロップ地点に残る)
fn snap_back(id: isize) {
    let home = APP.with(|a| {
        a.borrow().frames.iter().find(|f| f.dock.panel_of(id).is_some()).map(|f| f.hwnd)
    });
    if let Some(h) = home {
        crate::frame::reflow(h);
    }
}

/// 全フレームから undock を試みる。ドック済みだった場合 orig を返し、そのフレームを再配置
fn undock_from_any_frame(id: isize) -> Option<Rect> {
    let mut result = None;
    let mut reflow_hwnd = None;
    APP.with(|a| {
        let mut app = a.borrow_mut();
        for f in app.frames.iter_mut() {
            if let Some(orig) = f.dock.undock(id) {
                result = Some(orig);
                reflow_hwnd = Some(f.hwnd);
                break;
            }
        }
    });
    if let Some(h) = reflow_hwnd {
        crate::frame::reflow(h);
    }
    result
}

/// 仕様8: 消えたウィンドウは台帳から除去(もう存在しないので復元はしない)
fn on_destroyed(target: HWND) {
    let id = target.0 as isize;
    let mut reflow_hwnd = None;
    APP.with(|a| {
        let mut app = a.borrow_mut();
        for f in app.frames.iter_mut() {
            if f.dock.on_destroyed(id) {
                reflow_hwnd = Some(f.hwnd);
                break;
            }
        }
    });
    if let Some(h) = reflow_hwnd {
        crate::frame::reflow(h);
    }
}

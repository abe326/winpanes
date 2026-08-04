#![cfg(windows)]
use crate::config::FrameConfig;
use crate::dock::DockManager;
use crate::layout::{panel_areas, PanelArea, Preset, Rect, HEADER_H, TOOLBAR_H};
use crate::win_util::*;
use std::cell::RefCell;
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::Input::KeyboardAndMouse::{TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT};
use windows::Win32::UI::WindowsAndMessaging::*;

pub const FRAME_CLASS: PCWSTR = w!("WndPanelFrame");

/// ツールバー・ヘッダー上に置くボタン。描画とヒットテストが同じ定義を共有する
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ButtonId {
    Preset(Preset),
    ToolMax,
    ToolClose,
    PanelMax(usize),
    /// パネル最大化中のみツールバーに出る復帰ボタン
    /// (最大化ウィンドウがパネルヘッダーを覆い、ヘッダー上のボタンが押せなくなるため)
    PanelRestore,
}

pub struct FrameState {
    pub hwnd: HWND,
    pub preset: Preset,
    pub dock: DockManager<isize>, // HWND生値(isize)をIDに使う
    pub restore_rect: Rect,       // 非最大化時のウィンドウ矩形(config保存にも使う)
    pub maximized: bool,
    pub hover: Option<ButtonId>,
}

pub struct DragCtx {
    pub target: isize,     // ドラッグ中のHWND生値
    pub denied: bool,      // 取り込み不可(赤ハイライト)
    pub from_locked: bool, // ロック中パネルの占有者(ドロップ時にスナップバック)
}

pub struct App {
    pub frames: Vec<FrameState>,
    pub theme: Theme,
    pub app_hwnd: HWND,
    pub overlay: HWND,
    pub dragging: Option<DragCtx>,
    /// 最後に操作されたフレーム。トレイメニューのプリセット切替の適用先
    pub last_active: HWND,
    /// 終了処理中は保存を止める(フレームを畳む過程で設定を空にしないため)
    pub suppress_save: bool,
}

thread_local! {
    pub static APP: RefCell<App> = RefCell::new(App {
        frames: Vec::new(),
        theme: detect_theme(),
        app_hwnd: HWND::default(),
        overlay: HWND::default(),
        dragging: None,
        last_active: HWND::default(),
        suppress_save: false,
    });

    static SAVE_HOOK: RefCell<Option<fn()>> = const { RefCell::new(None) };
}

/// main から設定保存関数を注入する(frame -> main の循環参照を避ける)
pub fn set_save_hook(f: fn()) {
    SAVE_HOOK.with(|h| *h.borrow_mut() = Some(f));
}

pub fn request_save() {
    if APP.with(|a| a.borrow().suppress_save) {
        return;
    }
    let f = SAVE_HOOK.with(|h| *h.borrow());
    if let Some(f) = f {
        f();
    }
}

pub fn register_class() {
    unsafe {
        let wc = WNDCLASSW {
            style: CS_HREDRAW | CS_VREDRAW | CS_DBLCLKS,
            lpfnWndProc: Some(frame_wndproc),
            hInstance: GetModuleHandleW(None).unwrap().into(),
            hCursor: LoadCursorW(None, IDC_ARROW).unwrap(),
            lpszClassName: FRAME_CLASS,
            ..Default::default()
        };
        RegisterClassW(&wc);
    }
}

pub fn create(cfg: &FrameConfig) -> HWND {
    unsafe {
        let hwnd = CreateWindowExW(
            WINDOW_EX_STYLE::default(),
            FRAME_CLASS,
            w!("Winpanes"),
            // 枠なし+リサイズ境界。タイトルバーは自前ツールバーで代替(仕様6)
            WS_POPUP | WS_THICKFRAME | WS_MINIMIZEBOX,
            cfg.x,
            cfg.y,
            cfg.width,
            cfg.height,
            None,
            None,
            Some(GetModuleHandleW(None).unwrap().into()),
            None,
        )
        .expect("frame creation failed");
        APP.with(|a| {
            a.borrow_mut().frames.push(FrameState {
                hwnd,
                preset: cfg.preset,
                dock: DockManager::new(cfg.preset.panel_count()),
                restore_rect: Rect { x: cfg.x, y: cfg.y, w: cfg.width, h: cfg.height },
                maximized: false,
                hover: None,
            });
        });
        let _ = ShowWindow(hwnd, SW_SHOW);
        if cfg.maximized {
            toggle_tool_maximize(hwnd);
        }
        hwnd
    }
}

// ---------------------------------------------------------------- 矩形計算

/// クライアント矩形(クライアント座標、物理px)
fn client_rect(hwnd: HWND) -> Rect {
    let mut r = RECT::default();
    unsafe {
        let _ = GetClientRect(hwnd, &mut r);
    }
    to_rect(r)
}

/// ツールバーを除いたパネル領域(クライアント座標)
pub fn body_area(hwnd: HWND) -> Rect {
    let c = client_rect(hwnd);
    let tb = dpi_scale(hwnd, TOOLBAR_H);
    Rect { x: 0, y: tb, w: c.w, h: (c.h - tb).max(0) }
}

pub fn panels_client(state: &FrameState) -> Vec<PanelArea> {
    panel_areas(state.preset, body_area(state.hwnd), dpi_scale(state.hwnd, HEADER_H))
}

fn client_origin(hwnd: HWND) -> POINT {
    let mut origin = POINT { x: 0, y: 0 };
    unsafe {
        let _ = ClientToScreen(hwnd, &mut origin);
    }
    origin
}

/// スクリーン座標版(ドロップ判定・ウィンドウ配置用)
pub fn panels_screen(state: &FrameState) -> Vec<PanelArea> {
    let origin = client_origin(state.hwnd);
    panels_client(state)
        .into_iter()
        .map(|p| PanelArea {
            header: offset(p.header, origin.x, origin.y),
            body: offset(p.body, origin.x, origin.y),
        })
        .collect()
}

/// パネル領域全体のスクリーン矩形(パネル最大化の目標矩形)
pub fn screen_body_rect(f: &FrameState) -> Rect {
    let origin = client_origin(f.hwnd);
    let b = body_area(f.hwnd);
    Rect { x: b.x + origin.x, y: b.y + origin.y, w: b.w, h: b.h }
}

fn offset(r: Rect, dx: i32, dy: i32) -> Rect {
    Rect { x: r.x + dx, y: r.y + dy, ..r }
}

/// ボタン矩形(クライアント座標)。描画とヒットテストの両方が使う唯一の定義
pub fn button_rects(state: &FrameState) -> Vec<(ButtonId, Rect)> {
    let hwnd = state.hwnd;
    let c = client_rect(hwnd);
    let tb = dpi_scale(hwnd, TOOLBAR_H);
    let bw = dpi_scale(hwnd, 46); // キャプションボタン幅(Windows標準に近い)
    let pw = dpi_scale(hwnd, 72); // プリセットボタン幅
    let mut v = vec![
        (ButtonId::ToolClose, Rect { x: c.w - bw, y: 0, w: bw, h: tb }),
        (ButtonId::ToolMax, Rect { x: c.w - bw * 2, y: 0, w: bw, h: tb }),
    ];
    if state.dock.maximized_panel().is_some() {
        v.push((ButtonId::PanelRestore, Rect { x: c.w - bw * 3, y: 0, w: bw, h: tb }));
    }
    for (i, p) in [Preset::Grid2x2, Preset::Cols2, Preset::Rows2].into_iter().enumerate() {
        v.push((
            ButtonId::Preset(p),
            Rect { x: dpi_scale(hwnd, 8) + pw * i as i32, y: 0, w: pw, h: tb },
        ));
    }
    // パネルヘッダー右端の最大化ボタン
    let hb = dpi_scale(hwnd, 40);
    for (i, panel) in panels_client(state).iter().enumerate() {
        v.push((
            ButtonId::PanelMax(i),
            Rect {
                x: panel.header.x + panel.header.w - hb,
                y: panel.header.y,
                w: hb,
                h: panel.header.h,
            },
        ));
    }
    v
}

pub fn button_at(state: &FrameState, x: i32, y: i32) -> Option<ButtonId> {
    button_rects(state).into_iter().find(|(_, r)| r.contains(x, y)).map(|(b, _)| b)
}

// ---------------------------------------------------------------- 状態アクセス

pub fn with_frame(hwnd: HWND, f: impl FnOnce(&mut FrameState)) {
    APP.with(|a| {
        if let Some(fr) = a.borrow_mut().frames.iter_mut().find(|f| f.hwnd == hwnd) {
            f(fr);
        }
    });
}

pub fn with_frame_ret<T>(hwnd: HWND, f: impl FnOnce(&FrameState) -> T) -> Option<T> {
    APP.with(|a| a.borrow().frames.iter().find(|fr| fr.hwnd == hwnd).map(f))
}

// ---------------------------------------------------------------- wndproc

unsafe extern "system" fn frame_wndproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_NCHITTEST => hit_test(hwnd, lparam),
        // 非クライアント領域なし(枠・キャプションは自前描画)
        WM_NCCALCSIZE if wparam.0 != 0 => LRESULT(0),
        WM_PAINT => {
            paint(hwnd);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        WM_MOUSEMOVE => {
            on_mouse_move(hwnd, lparam);
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            set_hover(hwnd, None);
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            on_click(hwnd, lparam);
            LRESULT(0)
        }
        WM_NCLBUTTONDBLCLK if wparam.0 as u32 == HTCAPTION => {
            toggle_tool_maximize(hwnd);
            LRESULT(0)
        }
        // フレームはアクティブ化しない。アクティブ化の既定動作(最前面への引き上げ)が
        // ドック済みウィンドウを覆う原因のため、根元から断つ。
        // ボタン・ドラッグ移動・リサイズはアクティブ化なしで動作する
        WM_MOUSEACTIVATE => {
            raise_group(hwnd);
            LRESULT(MA_NOACTIVATE as isize)
        }
        // フレームがZオーダー上で引き上げられる場合、行き先を
        // 「ドック済みウィンドウ群の直下」へ書き換えて前へ出ることを事前に阻止する。
        // 自ウィンドウの変更は同一スレッドで同期的に確定するため競合しない
        // (他プロセスのウィンドウを事後に積み直す方式は SetWindowPos の
        //  非同期ポストによりアクティブ化処理との競合に負ける)
        WM_WINDOWPOSCHANGING => {
            let wp = unsafe { &mut *(lparam.0 as *mut WINDOWPOS) };
            if !wp.flags.contains(SWP_NOZORDER) {
                if let Some(bottom) = bottom_docked(hwnd) {
                    wp.hwndInsertAfter = bottom;
                }
            }
            unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) }
        }
        // 移動・リサイズ中のリアルタイム追従(仕様4)
        WM_MOVING | WM_SIZING => {
            reflow(hwnd);
            LRESULT(1)
        }
        WM_SIZE | WM_MOVE => {
            reflow(hwnd);
            LRESULT(0)
        }
        WM_EXITSIZEMOVE => {
            with_frame(hwnd, |f| {
                if !f.maximized {
                    if let Some(r) = window_rect(hwnd) {
                        f.restore_rect = r;
                    }
                }
            });
            request_save();
            LRESULT(0)
        }
        WM_DPICHANGED => {
            let suggested = unsafe { &*(lparam.0 as *const RECT) };
            unsafe {
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    suggested.left,
                    suggested.top,
                    suggested.right - suggested.left,
                    suggested.bottom - suggested.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            reflow(hwnd);
            LRESULT(0)
        }
        WM_CLOSE => {
            close_frame(hwnd);
            LRESULT(0)
        }
        WM_DESTROY => LRESULT(0),
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

/// ツールバー(ボタン以外)= HTCAPTION(OSにドラッグ移動を任せる)。端8pxはリサイズ
fn hit_test(hwnd: HWND, lparam: LPARAM) -> LRESULT {
    let (sx, sy) = (lparam.0 as i16 as i32, (lparam.0 >> 16) as i16 as i32);
    let mut pt = POINT { x: sx, y: sy };
    unsafe {
        let _ = ScreenToClient(hwnd, &mut pt);
    }
    let c = client_rect(hwnd);
    let m = 8;
    let maximized = with_frame_ret(hwnd, |f| f.maximized).unwrap_or(false);
    // 最大化中は端ドラッグでのリサイズを無効化(復帰ボタンで戻す)
    let (left, right, top, bottom) = if maximized {
        (false, false, false, false)
    } else {
        (pt.x < m, pt.x >= c.w - m, pt.y < m, pt.y >= c.h - m)
    };
    let ht = match (left, right, top, bottom) {
        (true, _, true, _) => HTTOPLEFT,
        (_, true, true, _) => HTTOPRIGHT,
        (true, _, _, true) => HTBOTTOMLEFT,
        (_, true, _, true) => HTBOTTOMRIGHT,
        (true, ..) => HTLEFT,
        (_, true, ..) => HTRIGHT,
        (_, _, true, _) => HTTOP,
        (_, _, _, true) => HTBOTTOM,
        _ => {
            let on_button =
                with_frame_ret(hwnd, |f| button_at(f, pt.x, pt.y).is_some()).unwrap_or(false);
            let in_toolbar = pt.y < dpi_scale(hwnd, TOOLBAR_H);
            if in_toolbar && !on_button {
                HTCAPTION
            } else {
                HTCLIENT
            }
        }
    };
    LRESULT(ht as isize)
}

// ---------------------------------------------------------------- 描画

/// Segoe Fluent Icons のグリフ(Win10 では Segoe MDL2 Assets にフォールバック)
const GLYPH_MAXIMIZE: u16 = 0xE922;
const GLYPH_RESTORE: u16 = 0xE923;
const GLYPH_CLOSE: u16 = 0xE8BB;

fn create_font(hwnd: HWND, face: PCWSTR, logical_h: i32) -> HFONT {
    unsafe {
        CreateFontW(
            -dpi_scale(hwnd, logical_h),
            0,
            0,
            0,
            FW_NORMAL.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            (DEFAULT_PITCH.0 | FF_DONTCARE.0) as u32,
            face,
        )
    }
}

/// アイコンフォント。Win11 は Segoe Fluent Icons、無ければ Segoe MDL2 Assets
fn create_icon_font(hwnd: HWND) -> HFONT {
    let f = create_font(hwnd, w!("Segoe Fluent Icons"), 12);
    if !f.is_invalid() && font_face_matches(f, "Segoe Fluent Icons") {
        return f;
    }
    unsafe {
        let _ = DeleteObject(f.into());
    }
    create_font(hwnd, w!("Segoe MDL2 Assets"), 12)
}

/// GDI がフォント名を代替した場合を検出する(要求どおりの字体かの確認)
fn font_face_matches(font: HFONT, expected: &str) -> bool {
    unsafe {
        let dc = CreateCompatibleDC(None);
        let old = SelectObject(dc, font.into());
        let mut buf = [0u16; 64];
        let len = GetTextFaceW(dc, Some(&mut buf)) as usize;
        SelectObject(dc, old);
        let _ = DeleteDC(dc);
        if len == 0 {
            return false;
        }
        let name = String::from_utf16_lossy(&buf[..len.saturating_sub(1)]);
        name == expected
    }
}

fn fill(dc: HDC, r: Rect, color: COLORREF) {
    unsafe {
        let brush = CreateSolidBrush(color);
        let wr = to_win_rect(r);
        FillRect(dc, &wr, brush);
        let _ = DeleteObject(brush.into());
    }
}

fn draw_text(dc: HDC, r: Rect, text: &str, color: COLORREF, flags: DRAW_TEXT_FORMAT) {
    unsafe {
        SetTextColor(dc, color);
        SetBkMode(dc, TRANSPARENT);
        let mut wide: Vec<u16> = text.encode_utf16().collect();
        let mut wr = to_win_rect(r);
        DrawTextW(dc, &mut wide, &mut wr, flags);
    }
}

fn draw_glyph(dc: HDC, r: Rect, glyph: u16, color: COLORREF) {
    unsafe {
        SetTextColor(dc, color);
        SetBkMode(dc, TRANSPARENT);
        let mut wide = [glyph];
        let mut wr = to_win_rect(r);
        DrawTextW(dc, &mut wide, &mut wr, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
    }
}

fn window_title(hwnd_raw: isize) -> String {
    unsafe {
        let h = HWND(hwnd_raw as *mut _);
        let mut buf = [0u16; 256];
        let len = GetWindowTextW(h, &mut buf) as usize;
        String::from_utf16_lossy(&buf[..len])
    }
}

fn paint(hwnd: HWND) {
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut ps);
        let c = client_rect(hwnd);
        // ダブルバッファ(ちらつき防止)
        let mem = CreateCompatibleDC(Some(hdc));
        let bmp = CreateCompatibleBitmap(hdc, c.w.max(1), c.h.max(1));
        let old = SelectObject(mem, bmp.into());
        let theme = APP.with(|a| a.borrow().theme);
        with_frame_ret(hwnd, |f| draw_frame(mem, c, f, &theme));
        let _ = BitBlt(hdc, 0, 0, c.w, c.h, Some(mem), 0, 0, SRCCOPY);
        SelectObject(mem, old);
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(mem);
        let _ = EndPaint(hwnd, &ps);
    }
}

fn draw_frame(dc: HDC, client: Rect, f: &FrameState, theme: &Theme) {
    let hwnd = f.hwnd;
    let tb = dpi_scale(hwnd, TOOLBAR_H);
    let hair = dpi_scale(hwnd, 1).max(1);

    // 1. 背景
    fill(dc, client, theme.bg);
    // 2. ツールバー帯 + 下辺ボーダー
    fill(dc, Rect { x: 0, y: 0, w: client.w, h: tb }, theme.toolbar);
    fill(dc, Rect { x: 0, y: tb - hair, w: client.w, h: hair }, theme.border);

    let text_font = create_font(hwnd, w!("Segoe UI"), 12);
    let icon_font = create_icon_font(hwnd);

    // 3. パネル(ヘッダー帯・タイトル・境界線)
    unsafe {
        SelectObject(dc, text_font.into());
    }
    let panels = panels_client(f);
    for (i, p) in panels.iter().enumerate() {
        fill(dc, p.header, theme.header);
        fill(
            dc,
            Rect { x: p.header.x, y: p.header.y + p.header.h - hair, w: p.header.w, h: hair },
            theme.border,
        );
        let pad = dpi_scale(hwnd, 10);
        let btn_w = dpi_scale(hwnd, 40);
        let title_rect = Rect {
            x: p.header.x + pad,
            y: p.header.y,
            w: (p.header.w - btn_w - pad * 2).max(0),
            h: p.header.h,
        };
        match f.dock.occupant(i) {
            Some(id) => draw_text(
                dc,
                title_rect,
                &window_title(id),
                theme.text,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS,
            ),
            None => draw_text(
                dc,
                title_rect,
                "(空)",
                theme.text_dim,
                DT_LEFT | DT_VCENTER | DT_SINGLELINE,
            ),
        }
        // パネル右辺・下辺の境界線
        if p.body.x + p.body.w < client.w {
            fill(
                dc,
                Rect { x: p.header.x + p.header.w - hair, y: p.header.y, w: hair, h: p.header.h + p.body.h },
                theme.border,
            );
        }
        if p.body.y + p.body.h < client.h {
            fill(
                dc,
                Rect { x: p.body.x, y: p.body.y + p.body.h - hair, w: p.body.w, h: hair },
                theme.border,
            );
        }
    }

    // 4. ボタン(プリセット=テキスト、キャプション/パネル最大化=アイコングリフ)
    for (id, r) in button_rects(f) {
        let hovered = f.hover == Some(id);
        match id {
            ButtonId::Preset(p) => {
                if hovered {
                    fill(dc, r, theme.hover);
                }
                unsafe {
                    SelectObject(dc, text_font.into());
                }
                draw_text(dc, r, p.label(), theme.text, DT_CENTER | DT_VCENTER | DT_SINGLELINE);
                if f.preset == p {
                    // 選択中はアクセントカラーの下線(仕様6: 装飾は最小限)
                    let ul = dpi_scale(hwnd, 2).max(1);
                    fill(dc, Rect { x: r.x, y: r.y + r.h - ul - hair, w: r.w, h: ul }, theme.accent);
                }
            }
            ButtonId::ToolClose => {
                if hovered {
                    fill(dc, r, theme.denied);
                }
                unsafe {
                    SelectObject(dc, icon_font.into());
                }
                let color = if hovered { COLORREF(0x00FFFFFF) } else { theme.text };
                draw_glyph(dc, r, GLYPH_CLOSE, color);
            }
            ButtonId::ToolMax => {
                if hovered {
                    fill(dc, r, theme.hover);
                }
                unsafe {
                    SelectObject(dc, icon_font.into());
                }
                let g = if f.maximized { GLYPH_RESTORE } else { GLYPH_MAXIMIZE };
                draw_glyph(dc, r, g, theme.text);
            }
            ButtonId::PanelRestore => {
                if hovered {
                    fill(dc, r, theme.hover);
                }
                unsafe {
                    SelectObject(dc, icon_font.into());
                }
                draw_glyph(dc, r, GLYPH_RESTORE, theme.accent);
            }
            ButtonId::PanelMax(i) => {
                if f.dock.occupant(i).is_none() {
                    continue; // 空パネルにはボタンを出さない
                }
                if hovered {
                    fill(dc, r, theme.hover);
                }
                unsafe {
                    SelectObject(dc, icon_font.into());
                }
                let g =
                    if f.dock.maximized_panel() == Some(i) { GLYPH_RESTORE } else { GLYPH_MAXIMIZE };
                draw_glyph(dc, r, g, theme.text_dim);
            }
        }
    }

    unsafe {
        let _ = DeleteObject(text_font.into());
        let _ = DeleteObject(icon_font.into());
    }
}

// ---------------------------------------------------------------- 入力

fn set_hover(hwnd: HWND, b: Option<ButtonId>) {
    let changed = with_frame_ret(hwnd, |f| f.hover != b).unwrap_or(false);
    if changed {
        with_frame(hwnd, |f| f.hover = b);
        unsafe {
            let _ = InvalidateRect(Some(hwnd), None, false);
        }
    }
}

fn on_mouse_move(hwnd: HWND, lparam: LPARAM) {
    let (x, y) = (lparam.0 as i16 as i32, (lparam.0 >> 16) as i16 as i32);
    let b = with_frame_ret(hwnd, |f| button_at(f, x, y)).flatten();
    set_hover(hwnd, b);
    unsafe {
        let mut tme = TRACKMOUSEEVENT {
            cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
            dwFlags: TME_LEAVE,
            hwndTrack: hwnd,
            dwHoverTime: 0,
        };
        let _ = TrackMouseEvent(&mut tme);
    }
}

fn on_click(hwnd: HWND, lparam: LPARAM) {
    let (x, y) = (lparam.0 as i16 as i32, (lparam.0 >> 16) as i16 as i32);
    let btn = with_frame_ret(hwnd, |f| button_at(f, x, y)).flatten();
    match btn {
        Some(ButtonId::ToolClose) => close_frame(hwnd),
        Some(ButtonId::ToolMax) => toggle_tool_maximize(hwnd),
        Some(ButtonId::Preset(p)) => set_preset(hwnd, p),
        Some(ButtonId::PanelMax(i)) => toggle_panel_maximize(hwnd, i),
        Some(ButtonId::PanelRestore) => {
            if let Some(p) = with_frame_ret(hwnd, |f| f.dock.maximized_panel()).flatten() {
                toggle_panel_maximize(hwnd, p);
            }
        }
        None => {}
    }
}

// ---------------------------------------------------------------- 操作

/// 仕様4: ツール最大化 = モニタ作業領域全体(タスクバーは覆わない)
pub fn toggle_tool_maximize(hwnd: HWND) {
    unsafe {
        let maximized = with_frame_ret(hwnd, |f| f.maximized).unwrap_or(false);
        if maximized {
            let Some(r) = with_frame_ret(hwnd, |f| f.restore_rect) else { return };
            with_frame(hwnd, |f| f.maximized = false);
            let _ = SetWindowPos(hwnd, None, r.x, r.y, r.w, r.h, SWP_NOZORDER);
        } else {
            if let Some(cur) = window_rect(hwnd) {
                with_frame(hwnd, |f| {
                    f.restore_rect = cur;
                    f.maximized = true;
                });
            }
            let mon = MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST);
            let mut mi = MONITORINFO {
                cbSize: std::mem::size_of::<MONITORINFO>() as u32,
                ..Default::default()
            };
            let _ = GetMonitorInfoW(mon, &mut mi);
            let wa = to_rect(mi.rcWork);
            let _ = SetWindowPos(hwnd, None, wa.x, wa.y, wa.w, wa.h, SWP_NOZORDER);
        }
    }
    reflow(hwnd);
    request_save();
}

/// 仕様4: パネル最大化 = フレームのパネル領域全体。他パネルのウィンドウは背後に残す
pub fn toggle_panel_maximize(hwnd: HWND, panel: usize) {
    let Some(id) = with_frame_ret(hwnd, |f| f.dock.occupant(panel)).flatten() else {
        return; // 空パネルは無効
    };
    let mut became_max = None;
    with_frame(hwnd, |f| became_max = f.dock.toggle_maximize(panel));
    if became_max == Some(true) {
        unsafe {
            let _ = SetWindowPos(
                HWND(id as *mut _),
                Some(HWND_TOP),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
    }
    reflow(hwnd);
}

pub fn set_preset(hwnd: HWND, p: Preset) {
    let mut released = Vec::new();
    with_frame(hwnd, |f| {
        f.preset = p;
        released = f.dock.set_panel_count(p.panel_count());
    });
    // あふれたウィンドウは元のサイズに復元(仕様4)
    for d in released {
        move_window_to(HWND(d.id as *mut _), d.orig);
    }
    reflow(hwnd);
    request_save();
}

/// パネル再計算 + ドック済みウィンドウの一括再配置 + 再描画
pub fn reflow(hwnd: HWND) {
    let targets =
        with_frame_ret(hwnd, |f| f.dock.target_rects(&panels_screen(f), screen_body_rect(f)));
    let dragging = APP.with(|a| a.borrow().dragging.as_ref().map(|d| d.target));
    if let Some(targets) = targets {
        let mut failed = Vec::new();
        unsafe {
            let mut hdwp = BeginDeferWindowPos(targets.len() as i32).unwrap_or_default();
            for (id, r) in &targets {
                if Some(*id) == dragging {
                    continue; // ユーザーがドラッグ中のウィンドウは奪わない
                }
                match DeferWindowPos(
                    hdwp,
                    HWND(*id as *mut _),
                    None,
                    r.x,
                    r.y,
                    r.w,
                    r.h,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                ) {
                    Ok(next) => hdwp = next,
                    Err(_) => failed.push(*id), // 仕様8: 移動できないウィンドウは解除
                }
            }
            let _ = EndDeferWindowPos(hdwp);
        }
        if !failed.is_empty() {
            with_frame(hwnd, |f| {
                for id in &failed {
                    let _ = f.dock.undock(*id);
                }
            });
        }
    }
    unsafe {
        let _ = InvalidateRect(Some(hwnd), None, true);
    }
}

/// 仕様4: フレームとドック済みウィンドウをグループとして手前へ。フレームは常に最背面。
/// 先にドック済みを前面へ出し、フレーム自身の引き上げは WM_WINDOWPOSCHANGING の
/// 書き換えによって「ドック済みの直下」に収まる
pub fn raise_group(hwnd: HWND) {
    APP.with(|a| a.borrow_mut().last_active = hwnd);
    let ids = docked_ids(hwnd);
    unsafe {
        for id in ids {
            let _ = SetWindowPos(
                HWND(id as *mut _),
                Some(HWND_TOP),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
        let _ = SetWindowPos(
            hwnd,
            Some(HWND_TOP),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
}

/// このフレームにドック済みのウィンドウID一覧
fn docked_ids(hwnd: HWND) -> Vec<isize> {
    with_frame_ret(hwnd, |f| {
        (0..f.dock.panel_count()).filter_map(|i| f.dock.occupant(i)).collect()
    })
    .unwrap_or_default()
}

/// このフレームのドック済みウィンドウのうち、Zオーダーが最も背面のものを返す。
/// デスクトップのトップレベルウィンドウを上から走査して最後に見つかったものが最背面
fn bottom_docked(hwnd: HWND) -> Option<HWND> {
    let ids = docked_ids(hwnd);
    if ids.is_empty() {
        return None;
    }
    let mut bottom = None;
    unsafe {
        let mut w = GetTopWindow(None).ok()?;
        while !w.is_invalid() {
            if ids.contains(&(w.0 as isize)) {
                bottom = Some(w);
            }
            match GetWindow(w, GW_HWNDNEXT) {
                Ok(next) => w = next,
                Err(_) => break,
            }
        }
    }
    bottom
}

/// 仕様6: 閉じるはそのフレームのみ。ドック済みは元の位置・サイズへ復元
pub fn close_frame(hwnd: HWND) {
    let mut restored = Vec::new();
    with_frame(hwnd, |f| restored = f.dock.drain_all());
    for d in restored {
        move_window_to(HWND(d.id as *mut _), d.orig);
    }
    APP.with(|a| a.borrow_mut().frames.retain(|f| f.hwnd != hwnd));
    unsafe {
        let _ = DestroyWindow(hwnd);
    }
    request_save();
    // 全フレームを閉じてもアプリは常駐継続。終了はトレイメニューから
}

pub const HEADER_H: i32 = 28; // 論理px(96dpi基準)。使用時にDPIスケールする
pub const TOOLBAR_H: i32 = 32;

#[derive(Clone, Copy, PartialEq, Eq, Debug, serde::Serialize, serde::Deserialize)]
pub enum Preset {
    #[serde(rename = "grid2x2")]
    Grid2x2,
    #[serde(rename = "cols2")]
    Cols2,
    #[serde(rename = "rows2")]
    Rows2,
}

impl Preset {
    pub fn panel_count(self) -> usize {
        match self {
            Preset::Grid2x2 => 4,
            Preset::Cols2 | Preset::Rows2 => 2,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Preset::Grid2x2 => "4分割",
            Preset::Cols2 => "縦2分割",
            Preset::Rows2 => "横2分割",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl Rect {
    pub fn contains(&self, px: i32, py: i32) -> bool {
        px >= self.x && px < self.x + self.w && py >= self.y && py < self.y + self.h
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PanelArea {
    pub header: Rect,
    pub body: Rect,
}

/// `area`: フレームのクライアント領域からツールバーを除いた矩形(物理px)
/// `header_h`: DPIスケール済みのヘッダー高さ(物理px)
pub fn panel_areas(preset: Preset, area: Rect, header_h: i32) -> Vec<PanelArea> {
    let (cols, rows) = match preset {
        Preset::Grid2x2 => (2, 2),
        Preset::Cols2 => (2, 1),
        Preset::Rows2 => (1, 2),
    };
    let xs = split_axis(area.x, area.w, cols);
    let ys = split_axis(area.y, area.h, rows);
    ys.iter()
        .flat_map(|&(y, h)| xs.iter().map(move |&(x, w)| Rect { x, y, w, h }))
        .map(|cell| {
            let hh = header_h.min(cell.h);
            PanelArea {
                header: Rect { x: cell.x, y: cell.y, w: cell.w, h: hh },
                body: Rect { x: cell.x, y: cell.y + hh, w: cell.w, h: (cell.h - hh).max(0) },
            }
        })
        .collect()
}

/// 最大化中ドラッグで復元する際の配置先(Windows標準の復元ドラッグ相当)。
/// カーソルの水平位置の比率を維持し、縦方向はカーソルがツールバー帯上に残る位置に置く。
/// `cur`: 現在(最大化中)の矩形 / `size`: 復元後の(幅, 高さ) /
/// `cx, cy`: カーソル位置 / `toolbar_h`: 物理pxのツールバー高さ
pub fn restore_drag_rect(cur: Rect, size: (i32, i32), cx: i32, cy: i32, toolbar_h: i32) -> Rect {
    let (w, h) = size;
    let fx = if cur.w > 0 { ((cx - cur.x) as f64 / cur.w as f64).clamp(0.0, 1.0) } else { 0.5 };
    let x = cx - (w as f64 * fx).round() as i32;
    let y = cy - (cy - cur.y).clamp(0, toolbar_h);
    Rect { x, y, w, h }
}

/// n等分し、割り切れない余りは最後のセルに寄せる
fn split_axis(start: i32, len: i32, n: i32) -> Vec<(i32, i32)> {
    let base = len / n;
    (0..n)
        .map(|i| {
            let s = start + base * i;
            let l = if i == n - 1 { len - base * (n - 1) } else { base };
            (s, l)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const AREA: Rect = Rect { x: 0, y: 0, w: 1000, h: 600 };

    #[test]
    fn cols2_splits_width_evenly() {
        let p = panel_areas(Preset::Cols2, AREA, 28);
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].header, Rect { x: 0, y: 0, w: 500, h: 28 });
        assert_eq!(p[0].body, Rect { x: 0, y: 28, w: 500, h: 572 });
        assert_eq!(p[1].body, Rect { x: 500, y: 28, w: 500, h: 572 });
    }

    #[test]
    fn rows2_splits_height_evenly() {
        let p = panel_areas(Preset::Rows2, AREA, 28);
        assert_eq!(p.len(), 2);
        assert_eq!(p[0].header, Rect { x: 0, y: 0, w: 1000, h: 28 });
        assert_eq!(p[1].header, Rect { x: 0, y: 300, w: 1000, h: 28 });
        assert_eq!(p[1].body, Rect { x: 0, y: 328, w: 1000, h: 272 });
    }

    #[test]
    fn grid2x2_is_row_major() {
        let p = panel_areas(Preset::Grid2x2, AREA, 28);
        assert_eq!(p.len(), 4);
        assert_eq!(p[0].header.x, 0); // 左上
        assert_eq!(p[1].header.x, 500); // 右上
        assert_eq!(p[1].header.y, 0);
        assert_eq!(p[2].header.x, 0); // 左下
        assert_eq!(p[2].header.y, 300);
        assert_eq!(p[3].header.x, 500); // 右下
        assert_eq!(p[3].header.y, 300);
    }

    #[test]
    fn odd_size_remainder_goes_to_last_cell() {
        let p = panel_areas(Preset::Cols2, Rect { x: 0, y: 0, w: 1001, h: 601 }, 28);
        assert_eq!(p[0].body.w, 500);
        assert_eq!(p[1].body.w, 501);
    }

    #[test]
    fn tiny_area_clamps_body_height_to_zero() {
        let p = panel_areas(Preset::Cols2, Rect { x: 0, y: 0, w: 100, h: 20 }, 28);
        assert_eq!(p[0].body.h, 0);
        assert_eq!(p[0].header.h, 20); // ヘッダーはセル高さでクリップ
    }

    #[test]
    fn restore_drag_keeps_cursor_ratio_horizontally() {
        // 最大化矩形 2000px 幅の中央 (x=1000) を掴んだら、復元後 800px 幅でも中央を掴む
        let cur = Rect { x: 0, y: 0, w: 2000, h: 1000 };
        let r = restore_drag_rect(cur, (800, 600), 1000, 16, 32);
        assert_eq!(r, Rect { x: 600, y: 0, w: 800, h: 600 });
    }

    #[test]
    fn restore_drag_clamps_to_edges() {
        let cur = Rect { x: 0, y: 0, w: 2000, h: 1000 };
        // 左端: カーソルが左端に残る
        assert_eq!(restore_drag_rect(cur, (800, 600), 0, 10, 32).x, 0);
        // 右端: カーソルが右端に残る
        assert_eq!(restore_drag_rect(cur, (800, 600), 2000, 10, 32).x, 1200);
        // 範囲外(モニタ跨ぎ等)でも比率は 0..1 にクランプ
        assert_eq!(restore_drag_rect(cur, (800, 600), -50, 10, 32).x, -50);
    }

    #[test]
    fn restore_drag_keeps_cursor_on_toolbar_vertically() {
        // 作業領域が y=100 から始まるモニタ: カーソルのY方向オフセットを維持
        let cur = Rect { x: 0, y: 100, w: 2000, h: 1000 };
        assert_eq!(restore_drag_rect(cur, (800, 600), 1000, 120, 32).y, 100);
        // オフセットがツールバー高を超える場合はツールバー帯上にクランプ
        assert_eq!(restore_drag_rect(cur, (800, 600), 1000, 200, 32).y, 168);
    }

    #[test]
    fn restore_drag_degenerate_width_centers() {
        let cur = Rect { x: 0, y: 0, w: 0, h: 0 };
        assert_eq!(restore_drag_rect(cur, (800, 600), 500, 0, 32).x, 100);
    }

    #[test]
    fn rect_contains_is_half_open() {
        let r = Rect { x: 10, y: 10, w: 100, h: 50 };
        assert!(r.contains(10, 10));
        assert!(r.contains(109, 59));
        assert!(!r.contains(110, 60));
        assert!(!r.contains(9, 10));
    }
}

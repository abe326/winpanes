use crate::layout::{PanelArea, Rect};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Docked<Id> {
    pub id: Id,
    pub orig: Rect, // ドック前のウィンドウ矩形(解除時の復元先)
}

#[derive(PartialEq, Eq, Debug)]
pub enum DockResult<Id> {
    /// 空きパネルへ新規配置
    Docked,
    /// ドック済みウィンドウが空きパネル(または同一パネル)へ移動
    Moved { from: usize },
    /// ドック済み同士の入れ替え。other は元いた側
    Swapped { from: usize, other: Id },
    /// 新規ウィンドウが占有パネルへ → 先住は解除(呼び出し側で orig へ復元すること)
    Replaced { freed: Docked<Id> },
    /// ロック中パネルが絡むため拒否(状態は一切変えない)
    Rejected,
}

pub struct DockManager<Id> {
    slots: Vec<Option<Docked<Id>>>,
    maximized: Option<usize>,
    locked: Vec<bool>,
}

impl<Id: Copy + Eq> DockManager<Id> {
    pub fn new(panel_count: usize) -> Self {
        Self {
            slots: (0..panel_count).map(|_| None).collect(),
            maximized: None,
            locked: vec![false; panel_count],
        }
    }

    pub fn panel_count(&self) -> usize {
        self.slots.len()
    }

    pub fn occupant(&self, panel: usize) -> Option<Id> {
        self.slots.get(panel).and_then(|s| s.map(|d| d.id))
    }

    pub fn panel_of(&self, id: Id) -> Option<usize> {
        self.slots.iter().position(|s| s.map(|d| d.id) == Some(id))
    }

    pub fn maximized_panel(&self) -> Option<usize> {
        self.maximized
    }

    pub fn is_locked(&self, panel: usize) -> bool {
        self.locked.get(panel).copied().unwrap_or(false)
    }

    /// パネルのロックをトグルし、新しい状態を返す。
    /// ロックはドック済みウィンドウの保護なので空パネルはロック不可
    pub fn toggle_lock(&mut self, panel: usize) -> bool {
        if self.occupant(panel).is_none() {
            return false;
        }
        if let Some(l) = self.locked.get_mut(panel) {
            *l = !*l;
            *l
        } else {
            false
        }
    }

    /// ドック操作。最大化中に呼ばれた場合は最大化を解除してから適用する(仕様4)。
    /// ロック中パネルが絡む場合は Rejected を返し、状態を一切変えない
    pub fn dock(&mut self, id: Id, panel: usize, orig: Rect) -> DockResult<Id> {
        if self.is_locked(panel) || self.panel_of(id).is_some_and(|p| self.is_locked(p)) {
            return DockResult::Rejected;
        }
        self.maximized = None;
        match self.panel_of(id) {
            Some(from) if from == panel => DockResult::Moved { from },
            Some(from) => {
                let moving = self.slots[from].take().expect("panel_of guaranteed occupancy");
                match self.slots[panel].take() {
                    Some(other) => {
                        let other_id = other.id;
                        self.slots[from] = Some(other);
                        self.slots[panel] = Some(moving);
                        DockResult::Swapped { from, other: other_id }
                    }
                    None => {
                        self.slots[panel] = Some(moving);
                        DockResult::Moved { from }
                    }
                }
            }
            None => match self.slots[panel].take() {
                Some(prev) => {
                    self.slots[panel] = Some(Docked { id, orig });
                    DockResult::Replaced { freed: prev }
                }
                None => {
                    self.slots[panel] = Some(Docked { id, orig });
                    DockResult::Docked
                }
            },
        }
    }

    pub fn undock(&mut self, id: Id) -> Option<Rect> {
        let panel = self.panel_of(id)?;
        if self.maximized == Some(panel) {
            self.maximized = None;
        }
        self.locked[panel] = false; // 空パネルにロックは残さない
        self.slots[panel].take().map(|d| d.orig)
    }

    pub fn on_destroyed(&mut self, id: Id) -> bool {
        self.undock(id).is_some()
    }

    pub fn toggle_maximize(&mut self, panel: usize) -> Option<bool> {
        self.occupant(panel)?;
        if self.maximized == Some(panel) {
            self.maximized = None;
            Some(false)
        } else {
            self.maximized = Some(panel);
            Some(true)
        }
    }

    /// プリセット変更でパネル数が変わる。あふれた分を解放して返す(仕様4)
    pub fn set_panel_count(&mut self, n: usize) -> Vec<Docked<Id>> {
        self.maximized = None;
        self.locked.resize(n, false);
        if n < self.slots.len() {
            self.slots.drain(n..).flatten().collect()
        } else {
            self.slots.resize_with(n, || None);
            Vec::new()
        }
    }

    /// 終了・フレームクローズ時の全解放(仕様4: 元の位置サイズへ復元)
    pub fn drain_all(&mut self) -> Vec<Docked<Id>> {
        self.maximized = None;
        self.locked.iter_mut().for_each(|l| *l = false);
        self.slots.iter_mut().filter_map(|s| s.take()).collect()
    }

    /// グループを前面へ引き上げる際の順序。後に挙げたものほど前面になるため、
    /// 最大化中のパネルの占有者は末尾(=最前面)に置く(仕様4: 他パネルは背後に残す)
    pub fn raise_order(&self) -> Vec<Id> {
        let mut ids: Vec<Id> = self
            .slots
            .iter()
            .enumerate()
            .filter(|(i, _)| self.maximized != Some(*i))
            .filter_map(|(_, s)| s.map(|d| d.id))
            .collect();
        if let Some(id) = self.maximized.and_then(|p| self.occupant(p)) {
            ids.push(id);
        }
        ids
    }

    /// 各ドック済みウィンドウの目標矩形。最大化パネルは frame_body 全体(仕様4)
    pub fn target_rects(&self, panels: &[PanelArea], frame_body: Rect) -> Vec<(Id, Rect)> {
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(i, s)| {
                s.as_ref().map(|d| {
                    let r = if self.maximized == Some(i) { frame_body } else { panels[i].body };
                    (d.id, r)
                })
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::layout::{panel_areas, Preset, Rect};

    const R1: Rect = Rect { x: 1, y: 1, w: 100, h: 100 };
    const R2: Rect = Rect { x: 2, y: 2, w: 200, h: 200 };

    #[test]
    fn dock_into_empty_panel() {
        let mut m = DockManager::new(4);
        assert_eq!(m.dock(10u32, 0, R1), DockResult::Docked);
        assert_eq!(m.occupant(0), Some(10));
        assert_eq!(m.panel_of(10), Some(0));
    }

    #[test]
    fn outside_drop_on_occupied_panel_replaces_and_frees() {
        // 仕様4: フレーム外からの占有パネルへのドロップは先住を解除して置き換え
        let mut m = DockManager::new(4);
        m.dock(10u32, 0, R1);
        let r = m.dock(20u32, 0, R2);
        assert_eq!(r, DockResult::Replaced { freed: Docked { id: 10, orig: R1 } });
        assert_eq!(m.occupant(0), Some(20));
        assert_eq!(m.panel_of(10), None);
    }

    #[test]
    fn docked_to_docked_swaps() {
        // 仕様4: ドック済み同士は入れ替え
        let mut m = DockManager::new(4);
        m.dock(10u32, 0, R1);
        m.dock(20u32, 1, R2);
        let r = m.dock(10u32, 1, R1);
        assert_eq!(r, DockResult::Swapped { from: 0, other: 20 });
        assert_eq!(m.occupant(0), Some(20));
        assert_eq!(m.occupant(1), Some(10));
    }

    #[test]
    fn docked_to_empty_moves() {
        let mut m = DockManager::new(4);
        m.dock(10u32, 0, R1);
        assert_eq!(m.dock(10u32, 2, R1), DockResult::Moved { from: 0 });
        assert_eq!(m.occupant(0), None);
        assert_eq!(m.occupant(2), Some(10));
    }

    #[test]
    fn undock_returns_original_rect() {
        // 仕様4: 解除時は元サイズ復元のために orig を返す
        let mut m = DockManager::new(4);
        m.dock(10u32, 0, R1);
        assert_eq!(m.undock(10), Some(R1));
        assert_eq!(m.panel_of(10), None);
        assert_eq!(m.undock(10), None);
    }

    #[test]
    fn destroyed_window_is_removed() {
        let mut m = DockManager::new(4);
        m.dock(10u32, 0, R1);
        assert!(m.on_destroyed(10));
        assert!(!m.on_destroyed(10));
        assert_eq!(m.occupant(0), None);
    }

    #[test]
    fn maximize_toggles_and_ignores_empty_panel() {
        let mut m = DockManager::new(4);
        assert_eq!(m.toggle_maximize(0), None); // 空パネルは無効
        m.dock(10u32, 0, R1);
        assert_eq!(m.toggle_maximize(0), Some(true));
        assert_eq!(m.maximized_panel(), Some(0));
        assert_eq!(m.toggle_maximize(0), Some(false));
        assert_eq!(m.maximized_panel(), None);
    }

    #[test]
    fn dock_cancels_maximize() {
        // 仕様4: 最大化中のドック系操作は最大化を解除してから適用
        let mut m = DockManager::new(4);
        m.dock(10u32, 0, R1);
        m.toggle_maximize(0);
        m.dock(20u32, 1, R2);
        assert_eq!(m.maximized_panel(), None);
    }

    #[test]
    fn undock_of_maximized_window_clears_maximize() {
        let mut m = DockManager::new(4);
        m.dock(10u32, 0, R1);
        m.toggle_maximize(0);
        m.undock(10);
        assert_eq!(m.maximized_panel(), None);
    }

    #[test]
    fn shrinking_panel_count_releases_overflow() {
        // 仕様4: プリセット変更であふれたウィンドウは解除
        let mut m = DockManager::new(4);
        m.dock(10u32, 1, R1);
        m.dock(20u32, 3, R2);
        let released = m.set_panel_count(2);
        assert_eq!(released, vec![Docked { id: 20, orig: R2 }]);
        assert_eq!(m.panel_count(), 2);
        assert_eq!(m.occupant(1), Some(10));
    }

    #[test]
    fn growing_panel_count_keeps_windows() {
        let mut m = DockManager::new(2);
        m.dock(10u32, 0, R1);
        assert!(m.set_panel_count(4).is_empty());
        assert_eq!(m.panel_count(), 4);
        assert_eq!(m.occupant(0), Some(10));
    }

    #[test]
    fn drain_all_returns_everything() {
        let mut m = DockManager::new(4);
        m.dock(10u32, 0, R1);
        m.dock(20u32, 3, R2);
        let mut all = m.drain_all();
        all.sort_by_key(|d| d.id);
        assert_eq!(all, vec![Docked { id: 10, orig: R1 }, Docked { id: 20, orig: R2 }]);
        assert_eq!(m.occupant(0), None);
    }

    #[test]
    fn target_rects_uses_body_and_maximized_uses_frame_body() {
        let mut m = DockManager::new(2);
        let panels = panel_areas(Preset::Cols2, Rect { x: 0, y: 0, w: 1000, h: 600 }, 28);
        let frame_body = Rect { x: 0, y: 0, w: 1000, h: 600 };
        m.dock(10u32, 0, R1);
        m.dock(20u32, 1, R2);
        let t = m.target_rects(&panels, frame_body);
        assert!(t.contains(&(10, panels[0].body)));
        assert!(t.contains(&(20, panels[1].body)));
        m.toggle_maximize(1);
        let t = m.target_rects(&panels, frame_body);
        assert!(t.contains(&(20, frame_body)));
        assert!(t.contains(&(10, panels[0].body)));
    }

    // ------------------------------------------------------------ ロック

    #[test]
    fn locked_panel_rejects_outside_drop() {
        let mut m = DockManager::new(4);
        m.dock(10u32, 0, R1);
        m.toggle_lock(0);
        assert_eq!(m.dock(20u32, 0, R2), DockResult::Rejected);
        assert_eq!(m.occupant(0), Some(10)); // 先住は不変
        assert_eq!(m.panel_of(20), None);
    }

    #[test]
    fn locked_panel_rejects_swap() {
        let mut m = DockManager::new(4);
        m.dock(10u32, 0, R1);
        m.dock(20u32, 1, R2);
        m.toggle_lock(1);
        assert_eq!(m.dock(10u32, 1, R1), DockResult::Rejected);
        assert_eq!(m.occupant(0), Some(10));
        assert_eq!(m.occupant(1), Some(20));
    }

    #[test]
    fn locked_source_panel_rejects_move_out() {
        // 完全ロック: ロック中パネルの占有者は他パネルへも動かせない
        let mut m = DockManager::new(4);
        m.dock(10u32, 0, R1);
        m.toggle_lock(0);
        assert_eq!(m.dock(10u32, 2, R1), DockResult::Rejected);
        assert_eq!(m.occupant(0), Some(10));
        assert_eq!(m.occupant(2), None);
    }

    #[test]
    fn empty_panel_cannot_be_locked() {
        // ロックはドック済みウィンドウの保護。空パネルはロック不可
        let mut m = DockManager::<u32>::new(4);
        assert!(!m.toggle_lock(3));
        assert!(!m.is_locked(3));
    }

    #[test]
    fn toggle_lock_toggles_and_reports() {
        let mut m = DockManager::new(2);
        m.dock(10u32, 0, R1);
        assert!(!m.is_locked(0));
        assert!(m.toggle_lock(0));
        assert!(m.is_locked(0));
        assert!(!m.toggle_lock(0));
        assert!(!m.is_locked(0));
    }

    #[test]
    fn raise_order_is_panel_order_without_maximize() {
        let mut m = DockManager::new(4);
        m.dock(10u32, 0, R1);
        m.dock(20u32, 1, R2);
        m.dock(30u32, 3, R1);
        assert_eq!(m.raise_order(), vec![10, 20, 30]);
    }

    #[test]
    fn raise_order_puts_maximized_window_last() {
        // 後に挙げたものほど前面になるため、最大化中のウィンドウは末尾であること
        let mut m = DockManager::new(4);
        m.dock(10u32, 0, R1);
        m.dock(20u32, 1, R2);
        m.dock(30u32, 3, R1);
        m.toggle_maximize(0);
        assert_eq!(m.raise_order(), vec![20, 30, 10]);
        m.toggle_maximize(0); // 解除でパネル順に戻る
        assert_eq!(m.raise_order(), vec![10, 20, 30]);
    }

    #[test]
    fn lock_does_not_block_maximize() {
        let mut m = DockManager::new(4);
        m.dock(10u32, 0, R1);
        m.toggle_lock(0);
        assert_eq!(m.toggle_maximize(0), Some(true));
        assert_eq!(m.maximized_panel(), Some(0));
        assert_eq!(m.toggle_maximize(0), Some(false));
    }

    #[test]
    fn rejected_dock_keeps_maximize() {
        // 拒否時は「dock は最大化を解除してから適用」も発動しない
        let mut m = DockManager::new(4);
        m.dock(10u32, 0, R1);
        m.dock(30u32, 1, R2);
        m.toggle_lock(1);
        m.toggle_maximize(0);
        assert_eq!(m.dock(20u32, 1, R2), DockResult::Rejected);
        assert_eq!(m.maximized_panel(), Some(0));
    }

    #[test]
    fn panel_count_change_resizes_locks() {
        let mut m = DockManager::new(4);
        m.dock(10u32, 1, R1);
        m.dock(20u32, 3, R2);
        m.toggle_lock(1);
        m.toggle_lock(3);
        m.set_panel_count(2);
        assert!(m.is_locked(1)); // 残るパネルのロックは維持
        assert!(!m.is_locked(3)); // あふれたロックは破棄(範囲外は false)
        m.set_panel_count(4);
        assert!(!m.is_locked(3)); // 拡大分は非ロック
    }

    #[test]
    fn undock_clears_lock() {
        // destroy・仕様8 の除去経路の保証: undock は成功し、空パネルにロックを残さない
        let mut m = DockManager::new(4);
        m.dock(10u32, 0, R1);
        m.toggle_lock(0);
        assert_eq!(m.undock(10), Some(R1));
        assert!(!m.is_locked(0));
    }

    #[test]
    fn drain_all_clears_locks() {
        // フレームクローズ時の全解放でロックも解除
        let mut m = DockManager::new(4);
        m.dock(10u32, 0, R1);
        m.toggle_lock(0);
        m.drain_all();
        assert!(!m.is_locked(0));
    }
}

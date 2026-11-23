use crate::config::{AppConfig, MainMenuEntry, MenuItemConfig};
use crate::modules::{BuiltinSubmenu, InstalledModules};

pub const PAGE_SIZE: usize = 8;
pub const GRID_COLUMNS: usize = 4;
pub const GRID_ROWS: usize = 3;
pub const GRID_SIZE: usize = 12;
pub const LIST_ROWS: usize = 6;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Mode {
    Main,
    Files,
    Apps,
    Windows,
    Clipboard,
    Settings,
    Custom(String),
}

impl Mode {
    pub fn from_id(id: &str) -> Self {
        match id.trim().to_ascii_lowercase().as_str() {
            "files" => Self::Files,
            "apps" => Self::Apps,
            "windows" => Self::Windows,
            "clipboard" => Self::Clipboard,
            "settings" => Self::Settings,
            _ => Self::Custom(id.trim().to_string()),
        }
    }

    pub fn id(&self) -> &str {
        match self {
            Self::Main => "main",
            Self::Files => "files",
            Self::Apps => "apps",
            Self::Windows => "windows",
            Self::Clipboard => "clipboard",
            Self::Settings => "settings",
            Self::Custom(s) => s.as_str(),
        }
    }

    pub fn title(&self) -> String {
        match self {
            Self::Main => "ReCircles".into(),
            Self::Files => "Files".into(),
            Self::Apps => "Apps".into(),
            Self::Windows => "Windows".into(),
            Self::Clipboard => "Clipboard".into(),
            Self::Settings => "Settings".into(),
            Self::Custom(s) => s.clone(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BuiltinIcon {
    #[default]
    Circle,
    Folder,
    Home,
    Pin,
    Drive,
    File,
    App,
    Window,
    Clipboard,
    Settings,
    Appearance,
    Theme,
    Scale,
    Layout,
    Material,
    Blur,
    Motion,
    Keybind,
    MainMenu,
    Add,
    Remove,
    Reset,
    Forget,
    Refresh,
    Check,
    ToggleOn,
    ToggleOff,
    Lock,
    Back,
    Disconnect,
    Palette,
    Image,
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub key: String,
    pub title: String,
    pub detail: String,
    pub icon: BuiltinIcon,
    pub icon_path: Option<String>,
    pub exe_icon_path: Option<String>,
    pub accent_override: Option<String>,
    pub active: bool,
    pub completion: Option<String>,
    pub payload: String,
    pub secondary_payload: Option<String>,
}

impl Entry {
    pub fn simple(key: impl Into<String>, title: impl Into<String>, icon: BuiltinIcon) -> Self {
        let k = key.into();
        let t = title.into();
        Self {
            key: k.clone(),
            title: t,
            detail: String::new(),
            icon,
            icon_path: None,
            exe_icon_path: None,
            accent_override: None,
            active: false,
            completion: None,
            payload: k,
            secondary_payload: None,
        }
    }
    pub fn completion_text(&self) -> Option<&str> {
        let candidate = self.completion.as_deref().unwrap_or(self.title.as_str());
        (!candidate.is_empty()).then_some(candidate)
    }
}

#[derive(Debug, Clone)]
struct UndoSnapshot {
    query: String,
    page: usize,
    grid_scroll: usize,
    list_scroll: usize,
    selected: usize,
}

#[derive(Debug, Clone, Default)]
pub struct MenuState {
    pub items: Vec<Entry>,
    pub filtered: Vec<usize>,
    pub query: String,
    pub page: usize,
    pub selected: Option<usize>, // index within `filtered`
    pub grid_scroll: usize,
    pub list_scroll: usize,
    pub preserve_order_when_empty: bool,
    undo_stack: Vec<UndoSnapshot>,
}

impl MenuState {
    pub fn set_items(&mut self, items: Vec<Entry>, preserve_order: bool) {
        self.items = items;
        self.preserve_order_when_empty = preserve_order;
        self.undo_stack.clear();
        self.refilter(true);
    }

    pub fn set_query(&mut self, query: String) {
        self.query = query;
        self.undo_stack.clear();
        self.refilter(true);
    }

    pub fn push_char(&mut self, c: char) {
        self.query.push(c);
        self.undo_stack.clear();
        self.refilter(true);
    }

    pub fn pop_char(&mut self) -> bool {
        if self.query.pop().is_some() {
            self.undo_stack.clear();
            self.refilter(true);
            true
        } else {
            false
        }
    }

    pub fn delete_word_backward(&mut self) -> bool {
        if self.query.is_empty() {
            return false;
        }
        let trimmed = self.query.trim_end_matches(['\\', '/', ' ']);
        if let Some(pos) = trimmed.rfind(['\\', '/', ' ']) {
            self.query.truncate(pos + 1);
        } else {
            self.query.clear();
        }
        self.undo_stack.clear();
        self.refilter(true);
        true
    }

    pub fn refilter(&mut self, reset_selection: bool) {
        let q = self.query.trim().to_ascii_lowercase();
        if q.is_empty() {
            self.filtered = (0..self.items.len()).collect();
        } else {
            let mut ranked: Vec<(usize, u8)> = self
                .items
                .iter()
                .enumerate()
                .filter_map(|(i, item)| score_match(item, &q).map(|score| (i, score)))
                .collect();
            if !self.preserve_order_when_empty {
                ranked.sort_by(|a, b| {
                    a.1.cmp(&b.1).then_with(|| a.0.cmp(&b.0))
                });
            }
            self.filtered = ranked.into_iter().map(|(i, _)| i).collect();
        }

        self.page = 0;
        self.grid_scroll = 0;
        self.list_scroll = 0;
        if self.filtered.is_empty() {
            self.selected = None;
        } else if reset_selection {
            self.selected = Some(0);
        } else if let Some(sel) = self.selected {
            self.selected = Some(sel.min(self.filtered.len() - 1));
        } else {
            self.selected = Some(0);
        }
    }

    pub fn visible_slice(&self, page_size: usize) -> &[usize] {
        if self.filtered.is_empty() || page_size == 0 {
            return &[];
        }
        let start = (self.page * page_size).min(self.filtered.len());
        let end = (start + page_size).min(self.filtered.len());
        &self.filtered[start..end]
    }

    pub fn visible_entries(&self, page_size: usize) -> Vec<&Entry> {
        self.visible_slice(page_size)
            .iter()
            .filter_map(|&idx| self.items.get(idx))
            .collect()
    }

    pub fn page_count(&self, page_size: usize) -> usize {
        if self.filtered.is_empty() || page_size == 0 {
            1
        } else {
            self.filtered.len().div_ceil(page_size)
        }
    }

    pub fn selected_entry(&self) -> Option<&Entry> {
        self.selected
            .and_then(|fi| self.filtered.get(fi))
            .and_then(|&idx| self.items.get(idx))
    }

    pub fn selected_visible_index(&self, page_size: usize) -> Option<usize> {
        let sel = self.selected?;
        let start = self.page * page_size;
        if sel >= start && sel < start + page_size {
            Some(sel - start)
        } else {
            None
        }
    }

    pub fn step_selection(&mut self, delta: isize, page_size: usize) {
        if self.filtered.is_empty() {
            self.selected = None;
            return;
        }
        let len = self.filtered.len() as isize;
        let next = match self.selected {
            Some(sel) => (sel as isize + delta).rem_euclid(len) as usize,
            None => {
                if delta >= 0 {
                    0
                } else {
                    (len - 1) as usize
                }
            }
        };
        self.selected = Some(next);
        if let Some(p) = next.checked_div(page_size) {
            self.page = p;
        }
    }

    pub fn select_visible(&mut self, vis_idx: usize, page_size: usize) {
        let abs = self.page * page_size + vis_idx;
        if abs < self.filtered.len() {
            self.selected = Some(abs);
        }
    }

    pub fn next_page(&mut self, page_size: usize) {
        let total = self.page_count(page_size);
        if total <= 1 {
            return;
        }
        self.page = (self.page + 1) % total;
        self.selected = Some((self.page * page_size).min(self.filtered.len().saturating_sub(1)));
    }

    pub fn prev_page(&mut self, page_size: usize) {
        let total = self.page_count(page_size);
        if total <= 1 {
            return;
        }
        self.page = (self.page + total - 1) % total;
        self.selected = Some((self.page * page_size).min(self.filtered.len().saturating_sub(1)));
    }

    pub fn grid_max_scroll(&self) -> usize {
        self.filtered
            .len()
            .div_ceil(GRID_COLUMNS)
            .saturating_sub(GRID_ROWS)
            * GRID_COLUMNS
    }

    pub fn grid_first_visible(&self) -> usize {
        let max_scroll = self.grid_max_scroll();
        (self.grid_scroll / GRID_COLUMNS * GRID_COLUMNS).min(max_scroll)
    }

    pub fn grid_visible_slice(&self) -> &[usize] {
        if self.filtered.is_empty() {
            return &[];
        }
        let start = self.grid_first_visible().min(self.filtered.len());
        let end = (start + GRID_SIZE).min(self.filtered.len());
        &self.filtered[start..end]
    }

    pub fn grid_visible_entries(&self) -> Vec<&Entry> {
        self.grid_visible_slice()
            .iter()
            .filter_map(|&idx| self.items.get(idx))
            .collect()
    }

    pub fn grid_selected_visible_index(&self) -> Option<usize> {
        let sel = self.selected?;
        let start = self.grid_first_visible();
        if sel >= start && sel < start + GRID_SIZE {
            Some(sel - start)
        } else {
            None
        }
    }

    pub fn select_grid_visible(&mut self, vis_idx: usize) {
        let abs = self.grid_first_visible() + vis_idx;
        if abs < self.filtered.len() {
            self.selected = Some(abs);
        }
    }

    pub fn scroll_grid_to(&mut self, first: usize) -> bool {
        let max_scroll = self.grid_max_scroll();
        let clamped = (first / GRID_COLUMNS * GRID_COLUMNS).min(max_scroll);
        if clamped != self.grid_scroll {
            self.grid_scroll = clamped;
            self.selected = None;
            true
        } else {
            false
        }
    }

    pub fn scroll_grid_rows(&mut self, delta_rows: isize) -> bool {
        let cur = self.grid_first_visible();
        let target = cur.saturating_add_signed(delta_rows * GRID_COLUMNS as isize);
        self.scroll_grid_to(target)
    }

    pub fn ensure_grid_selection_visible(&mut self) {
        let Some(sel) = self.selected else {
            self.grid_scroll = self.grid_first_visible();
            return;
        };
        let row = sel / GRID_COLUMNS;
        let max_first_row = self
            .filtered
            .len()
            .div_ceil(GRID_COLUMNS)
            .saturating_sub(GRID_ROWS);
        let cur_first_row = (self.grid_scroll / GRID_COLUMNS).min(max_first_row);
        let next_first_row = cur_first_row
            .clamp((row + 1).saturating_sub(GRID_ROWS), row)
            .min(max_first_row);
        self.grid_scroll = next_first_row * GRID_COLUMNS;
    }

    pub fn turn_grid_page(&mut self, delta: isize) -> bool {
        let last = self.grid_max_scroll();
        if last == 0 || delta == 0 {
            self.grid_scroll = 0;
            return false;
        }
        let cur = self.grid_first_visible();
        let dir = delta.signum();
        let next = match (cur, dir) {
            (0, -1) => last,
            (first, 1) if first >= last => 0,
            (first, d) => first
                .saturating_add_signed(d * GRID_SIZE as isize)
                .min(last),
        };
        let next = (next / GRID_COLUMNS * GRID_COLUMNS).min(last);
        let changed = next != self.grid_scroll;
        self.grid_scroll = next;
        if !self.filtered.is_empty() {
            self.selected = Some(next.min(self.filtered.len() - 1));
        }
        changed
    }

    pub fn move_grid(&mut self, dx: isize, dy: isize) {
        if self.filtered.is_empty() {
            self.selected = None;
            return;
        }
        let len = self.filtered.len();
        let cur = match self.selected {
            Some(sel) => sel.min(len - 1),
            None => {
                let first = self.grid_first_visible().min(len - 1);
                let start = if dx < 0 || dy < 0 {
                    (first as isize - 1).rem_euclid(len as isize) as usize
                } else {
                    first
                };
                self.selected = Some(start);
                self.ensure_grid_selection_visible();
                return;
            }
        };

        let next = if dx != 0 && dy == 0 {
            (cur as isize + dx).rem_euclid(len as isize) as usize
        } else {
            let total_rows = len.div_ceil(GRID_COLUMNS);
            let row = cur / GRID_COLUMNS;
            let col = cur % GRID_COLUMNS;
            let new_col = (col as isize + dx).rem_euclid(GRID_COLUMNS as isize) as usize;
            let new_row = (row as isize + dy).rem_euclid(total_rows as isize) as usize;
            (new_row * GRID_COLUMNS + new_col).min(len - 1)
        };
        self.selected = Some(next);
        self.ensure_grid_selection_visible();
    }

    pub fn list_max_scroll(&self) -> usize {
        self.filtered.len().saturating_sub(LIST_ROWS)
    }

    pub fn list_first_visible(&self) -> usize {
        self.list_scroll.min(self.list_max_scroll())
    }

    pub fn list_visible_slice(&self) -> &[usize] {
        if self.filtered.is_empty() {
            return &[];
        }
        let start = self.list_first_visible().min(self.filtered.len());
        let end = (start + LIST_ROWS).min(self.filtered.len());
        &self.filtered[start..end]
    }

    pub fn list_selected_visible_index(&self) -> Option<usize> {
        let sel = self.selected?;
        let start = self.list_first_visible();
        if sel >= start && sel < start + LIST_ROWS {
            Some(sel - start)
        } else {
            None
        }
    }

    pub fn select_list_visible(&mut self, vis_idx: usize) {
        let abs = self.list_first_visible() + vis_idx;
        if abs < self.filtered.len() {
            self.selected = Some(abs);
        }
    }

    pub fn scroll_list_to(&mut self, first: usize) -> bool {
        let clamped = first.min(self.list_max_scroll());
        if clamped != self.list_scroll {
            self.list_scroll = clamped;
            self.selected = None;
            true
        } else {
            false
        }
    }

    pub fn scroll_list_rows(&mut self, delta_rows: isize) -> bool {
        let cur = self.list_first_visible();
        let target = cur.saturating_add_signed(delta_rows);
        self.scroll_list_to(target)
    }

    pub fn turn_list_page(&mut self, delta: isize) -> bool {
        let last = self.list_max_scroll();
        if last == 0 || delta == 0 {
            self.list_scroll = 0;
            return false;
        }
        let cur = self.list_first_visible();
        let dir = delta.signum();
        let next = match (cur, dir) {
            (0, -1) => last,
            (first, 1) if first >= last => 0,
            (first, d) => first
                .saturating_add_signed(d * LIST_ROWS as isize)
                .min(last),
        };
        let changed = next != self.list_scroll;
        self.list_scroll = next;
        if !self.filtered.is_empty() {
            self.selected = Some(next.min(self.filtered.len() - 1));
        }
        changed
    }

    pub fn move_list(&mut self, delta: isize) {
        if self.filtered.is_empty() {
            return;
        }
        let len = self.filtered.len() as isize;
        let cur = match self.selected {
            Some(sel) => sel as isize,
            None => {
                let first = self.list_first_visible() as isize;
                let start = if delta < 0 {
                    (first - 1).rem_euclid(len) as usize
                } else {
                    first.rem_euclid(len) as usize
                };
                self.selected = Some(start);
                self.list_scroll = self
                    .list_scroll
                    .clamp((start + 1).saturating_sub(LIST_ROWS), start)
                    .min(self.list_max_scroll());
                return;
            }
        };
        let next = (cur + delta).rem_euclid(len) as usize;
        self.selected = Some(next);
        if next < self.list_scroll {
            self.list_scroll = next;
        } else if next >= self.list_scroll + LIST_ROWS {
            self.list_scroll = (next + 1 - LIST_ROWS).min(self.list_max_scroll());
        }
    }

    pub fn suggestion(&self, normalize_path_sep: bool) -> Option<(usize, &str)> {
        if self.query.is_empty() {
            return None;
        }
        let q = if normalize_path_sep {
            self.query.replace('/', "\\").to_lowercase()
        } else {
            self.query.to_lowercase()
        };
        let prefix_chars = self.query.chars().count();
        for (fi, &idx) in self.filtered.iter().enumerate() {
            let Some(entry) = self.items.get(idx) else {
                continue;
            };
            let Some(candidate) = entry.completion_text() else {
                continue;
            };
            let Some((end, _)) = candidate.char_indices().nth(prefix_chars) else {
                continue;
            };
            let prefix = if normalize_path_sep {
                candidate[..end].replace('/', "\\").to_lowercase()
            } else {
                candidate[..end].to_lowercase()
            };
            if prefix == q {
                let suffix = &candidate[end..];
                if !suffix.is_empty() {
                    return Some((fi, suffix));
                }
            }
        }
        None
    }

    pub fn accept_completion(&mut self, filtered_idx: usize) -> Option<String> {
        let item_idx = *self.filtered.get(filtered_idx)?;
        let comp = self.items.get(item_idx)?.completion_text()?.to_owned();
        if comp == self.query {
            return None;
        }
        self.undo_stack.push(UndoSnapshot {
            query: self.query.clone(),
            page: self.page,
            grid_scroll: self.grid_scroll,
            list_scroll: self.list_scroll,
            selected: self.selected.unwrap_or(0),
        });
        self.query = comp.clone();
        self.selected = Some(filtered_idx);
        self.page = filtered_idx / PAGE_SIZE;
        self.ensure_grid_selection_visible();
        if filtered_idx < self.list_scroll {
            self.list_scroll = filtered_idx;
        } else if filtered_idx >= self.list_scroll + LIST_ROWS {
            self.list_scroll = (filtered_idx + 1 - LIST_ROWS).min(self.list_max_scroll());
        }
        Some(comp)
    }

    pub fn undo_completion(&mut self) -> Option<String> {
        let snap = self.undo_stack.pop()?;
        self.query = snap.query.clone();
        self.refilter(false);
        self.page = snap.page;
        self.grid_scroll = snap.grid_scroll.min(self.grid_max_scroll());
        self.list_scroll = snap.list_scroll.min(self.list_max_scroll());
        if !self.filtered.is_empty() {
            self.selected = Some(snap.selected.min(self.filtered.len() - 1));
        }
        Some(snap.query)
    }

    pub fn clear_completion(&mut self) {
        self.undo_stack.clear();
    }

    pub fn status_text(&self, page_size: usize) -> String {
        let total = self.filtered.len();
        let pages = self.page_count(page_size);
        if !self.query.is_empty() {
            if total == 0 {
                return format!("No matches for '{}'", self.query);
            }
            if pages > 1 {
                return format!("{} ({total} · {}/{pages})", self.query, self.page + 1);
            }
            return format!("{} ({total})", self.query);
        }
        if pages > 1 {
            format!("{total} items · page {}/{pages}", self.page + 1)
        } else if total == 1 {
            "1 item".into()
        } else {
            format!("{total} items")
        }
    }
}

fn score_match(item: &Entry, q: &str) -> Option<u8> {
    let title = item.title.to_ascii_lowercase();
    if title == q {
        Some(0)
    } else if title.starts_with(q) {
        Some(1)
    } else if title
        .split(|c: char| !c.is_alphanumeric())
        .any(|w| w.starts_with(q))
    {
        Some(2)
    } else if title.contains(q) {
        Some(3)
    } else if !item.detail.is_empty() && item.detail.to_ascii_lowercase().contains(q) {
        Some(4)
    } else {
        None
    }
}

pub fn build_main_menu_entries(cfg: &AppConfig, installed: &InstalledModules) -> Vec<Entry> {
    cfg.main_menu
        .iter()
        .filter(|entry| {
            if let Some(sub) = BuiltinSubmenu::from_id(&entry.menu) {
                installed.is_installed(sub)
            } else {
                cfg.menus.contains_key(&entry.menu)
            }
        })
        .map(|entry| main_entry_to_menu_entry(entry, cfg))
        .collect()
}

fn main_entry_to_menu_entry(entry: &MainMenuEntry, cfg: &AppConfig) -> Entry {
    let id = &entry.menu;
    let icon = match id.as_str() {
        "files" => BuiltinIcon::Folder,
        "apps" => BuiltinIcon::App,
        "windows" => BuiltinIcon::Window,
        "clipboard" => BuiltinIcon::Clipboard,
        "settings" => BuiltinIcon::Settings,
        _ => BuiltinIcon::Circle,
    };
    let hotkey = cfg.keybinds.menu_hotkey(id).primary().map(ToOwned::to_owned);
    let mut e = Entry {
        key: id.clone(),
        title: entry.resolved_title(&cfg.menus),
        detail: hotkey.unwrap_or_default(),
        icon,
        icon_path: entry
            .icon
            .clone()
            .or_else(|| cfg.menus.get(id).and_then(|m| m.icon.clone())),
        exe_icon_path: None,
        accent_override: entry
            .accent
            .clone()
            .or_else(|| cfg.menus.get(id).and_then(|m| m.accent.clone())),
        active: false,
        completion: None,
        payload: id.clone(),
        secondary_payload: None,
    };
    if id == "clipboard" && e.icon_path.is_none() {
        // Rendered using crisp built-in vector clipboard icon (exact clipboard.svg geometry)
        e.icon = BuiltinIcon::Clipboard;
    }
    e
}

pub fn build_custom_menu_entries(items: &[MenuItemConfig]) -> Vec<Entry> {
    items
        .iter()
        .enumerate()
        .map(|(i, item)| Entry {
            key: format!("custom:{i}:{}", item.label),
            title: item.label.clone(),
            detail: item.detail.clone().unwrap_or_default(),
            icon: BuiltinIcon::Circle,
            icon_path: item.icon.clone(),
            exe_icon_path: None,
            accent_override: item.accent.clone(),
            active: false,
            completion: None,
            payload: i.to_string(),
            secondary_payload: item
                .right_click
                .as_ref()
                .map(|_| format!("right:{i}")),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    #[test]
    fn custom_menu_filtering_preserves_order_and_paging_reaches_all() {
        let mut state = MenuState::default();
        let items: Vec<Entry> = (0..18)
            .map(|i| Entry::simple(format!("k{i}"), format!("Item {i:02}"), BuiltinIcon::App))
            .collect();
        state.set_items(items, true);
        assert_eq!(state.page_count(PAGE_SIZE), 3);
        assert_eq!(state.visible_slice(PAGE_SIZE).len(), 8);

        state.next_page(PAGE_SIZE);
        assert_eq!(state.page, 1);
        state.next_page(PAGE_SIZE);
        assert_eq!(state.page, 2);
        assert_eq!(state.visible_slice(PAGE_SIZE).len(), 2);

        state.set_query("1".into());
        assert!(state.filtered.len() >= 9);
    }

    #[test]
    fn main_menu_entries_hide_uninstalled_optional_submenus() {
        let temp = std::env::temp_dir().join("recircles_menu_test_missing");
        let _ = std::fs::remove_dir_all(&temp);
        let installed = InstalledModules::detect_in(temp);
        let cfg = AppConfig::default_for_installed(&InstalledModules::all_for_tests(PathBuf::from(".")));
        let visible = build_main_menu_entries(&cfg, &installed);
        assert_eq!(visible.len(), 1);
        assert_eq!(visible[0].key, "settings");
    }

    #[test]
    fn grid_row_scrolling_clamps_for_13_to_23_and_111_items() {
        let make = |n: usize| {
            let mut st = MenuState::default();
            let items: Vec<Entry> = (0..n)
                .map(|i| Entry::simple(format!("k{i}"), format!("App {i}"), BuiltinIcon::App))
                .collect();
            st.set_items(items, true);
            st
        };

        // 12 items: 3 rows -> no scrolling needed
        let mut s12 = make(12);
        assert_eq!(s12.grid_max_scroll(), 0);
        assert!(!s12.scroll_grid_rows(1));
        assert!(!s12.turn_grid_page(1));
        assert_eq!(s12.grid_first_visible(), 0);

        // 13..=23 items: row-aligned max scroll matches (rows - GRID_ROWS) * GRID_COLUMNS
        for total in 13usize..=23 {
            let expected_max = (total.div_ceil(GRID_COLUMNS) - GRID_ROWS) * GRID_COLUMNS;
            let mut st = make(total);
            assert_eq!(st.grid_max_scroll(), expected_max, "total={total}");

            // Single-row wheel scroll steps by GRID_COLUMNS (4), never 12
            assert!(st.scroll_grid_rows(1));
            assert_eq!(st.grid_first_visible(), 4, "total={total}");
            assert_eq!(st.grid_visible_slice()[0], 4);

            // Clamping past the end never exceeds expected_max
            st.scroll_grid_to(total + 100);
            assert_eq!(st.grid_first_visible(), expected_max, "total={total}");

            // PageDown from 0 jumps up to expected_max (not 12 if max is 4 or 8), then wraps to 0
            st.scroll_grid_to(0);
            assert!(st.turn_grid_page(1));
            assert_eq!(st.grid_first_visible(), expected_max.min(GRID_SIZE));
            if expected_max <= GRID_SIZE {
                assert!(st.turn_grid_page(1));
                assert_eq!(st.grid_first_visible(), 0);
                // PageUp from 0 wraps to expected_max
                assert!(st.turn_grid_page(-1));
                assert_eq!(st.grid_first_visible(), expected_max);
            }
        }

        // 19 items: verify 1-row arrow navigation matching Linux select_entry
        let mut s19 = make(19);
        assert_eq!(s19.grid_max_scroll(), 8);
        assert_eq!(s19.selected, Some(0));
        s19.move_grid(0, 1); // 4
        s19.move_grid(0, 1); // 8
        assert_eq!(s19.grid_first_visible(), 0);
        s19.move_grid(0, 1); // 12 -> scrolls 1 row to 4
        assert_eq!((s19.selected, s19.grid_first_visible()), (Some(12), 4));
        s19.move_grid(0, 1); // 16 -> scrolls 1 row to 8 (bottom)
        assert_eq!((s19.selected, s19.grid_first_visible()), (Some(16), 8));
        assert_eq!(s19.grid_visible_slice().len(), 11); // 8..19
        s19.move_grid(0, 1); // wraps to 0 -> scrolls back to 0
        assert_eq!((s19.selected, s19.grid_first_visible()), (Some(0), 0));

        // 111 items (28 rows): max scroll is (28 - 3) * 4 = 100, NEVER 108!
        let mut s111 = make(111);
        assert_eq!(s111.grid_max_scroll(), 100);
        // Step PageDown repeatedly until bottom, then wrap
        let mut seen = vec![s111.grid_first_visible()];
        for _ in 0..10 {
            s111.turn_grid_page(1);
            seen.push(s111.grid_first_visible());
        }
        assert_eq!(seen, vec![0, 12, 24, 36, 48, 60, 72, 84, 96, 100, 0]);
        // PageUp from 0 lands cleanly on 100
        s111.turn_grid_page(-1);
        assert_eq!(s111.grid_first_visible(), 100);
        assert_eq!(s111.grid_visible_slice().len(), 11); // items 100..111
    }
}

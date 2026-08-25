mod lists;
mod local_tree;
mod lyrics;
mod overlays;
mod playback;
mod search_panels;
mod tracks;
mod visualizer;
mod welcome;

use ratatui::{
    Frame,
    layout::Rect,
    widgets::{List, ListItem, ListState},
};
use std::borrow::Cow;
use unicode_width::UnicodeWidthStr;

use super::{Focus, LIBRARY_ITEMS, LocalNode, PlaybackState, SearchPanel, Ui, UiState};

pub(super) fn clamp_text(text: &str, max_width: usize) -> Cow<'_, str> {
    if text.width() <= max_width {
        Cow::Borrowed(text)
    } else {
        let mut result = String::new();
        let mut w = 0;
        for ch in text.chars() {
            let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(1);
            if w + cw + 3 > max_width {
                break;
            }
            result.push(ch);
            w += cw;
        }
        result.push_str("...");
        Cow::Owned(result)
    }
}

pub(super) fn pad_right(text: &str, width: usize) -> String {
    let current = text.width();
    if current >= width {
        text.to_string()
    } else {
        format!("{}{}", text, " ".repeat(width - current))
    }
}

pub(super) fn fmt_duration(ms: u64) -> String {
    let s = ms / 1000;
    format!("{:>2}:{:02}", s / 60, s % 60)
}

pub(super) fn calculate_number_width(total: usize) -> usize {
    if total == 0 {
        return 1;
    }
    total.to_string().len()
}

pub(super) struct ListWindow<'a> {
    pub items: Vec<ListItem<'a>>,
    pub start: usize,
    pub selected: Option<usize>,
}

pub(super) fn build_list_window<'a, F>(
    total: usize,
    height: usize,
    list_state: &ListState,
    item_fn: F,
) -> ListWindow<'a>
where
    F: FnMut(usize) -> ListItem<'a>,
{
    let visible = height.max(1);
    let selected = list_state
        .selected()
        .map(|index| index.min(total.saturating_sub(1)));
    let selected_index = selected.unwrap_or(0);
    let start = selected_index
        .saturating_sub(visible / 2)
        .min(total.saturating_sub(visible));
    let end = (start + visible).min(total);
    let items = (start..end).map(item_fn).collect();
    let local_selected =
        selected.and_then(|index| (index >= start && index < end).then_some(index - start));

    ListWindow {
        items,
        start,
        selected: local_selected,
    }
}

pub(super) fn render_list_window<'a>(
    frame: &mut Frame,
    list: List<'a>,
    area: Rect,
    global_state: &mut ListState,
    start: usize,
    selected: Option<usize>,
) {
    let mut local_state = ListState::default().with_selected(selected);
    frame.render_stateful_widget(list, area, &mut local_state);
    *global_state.offset_mut() = start + local_state.offset();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn list_window_centers_global_selection() {
        let state = ListState::default().with_selected(Some(100));
        let window = build_list_window(2_240, 10, &state, |index| ListItem::new(index.to_string()));

        assert_eq!(window.start, 95);
        assert_eq!(window.items.len(), 10);
        assert_eq!(window.selected, Some(5));
    }

    #[test]
    fn list_window_clamps_to_list_edges() {
        let state = ListState::default().with_selected(Some(2_239));
        let window = build_list_window(2_240, 10, &state, |index| ListItem::new(index.to_string()));

        assert_eq!(window.start, 2_230);
        assert_eq!(window.items.len(), 10);
        assert_eq!(window.selected, Some(9));
    }
}

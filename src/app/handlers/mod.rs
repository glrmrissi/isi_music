mod dispatch;
mod dispatch_helpers;
mod enter;
mod enter_search;
mod key;
mod mouse;
mod navigation;
mod playback;
mod playlist_cmds;
mod search;

/// Map a mouse click (cx, cy) to a list item index, given the widget's outer
/// rect (including borders) and the list's scroll offset. Returns None if the
/// click is on the border or outside the list items.
fn click_to_list_index(
    rect: &ratatui::layout::Rect,
    cx: u16,
    cy: u16,
    total: usize,
    scroll_offset: usize,
) -> Option<usize> {
    if total == 0 {
        return None;
    }
    // Inner area: subtract 1px border on each side
    let inner_x = rect.x + 1;
    let inner_y = rect.y + 1;
    let inner_h = rect.height.saturating_sub(2) as usize;

    if inner_h == 0 {
        return None;
    }
    if cy < inner_y || cy >= inner_y + inner_h as u16 {
        return None;
    }
    let _ = inner_x;
    let _ = cx;

    let visible_row = (cy - inner_y) as usize;
    let global_index = scroll_offset + visible_row;
    if global_index >= total {
        return None;
    }
    Some(global_index)
}

/// Format milliseconds as MM:SS for the seek status message.
fn fmt_seek(ms: u64) -> String {
    let secs = ms / 1000;
    let m = secs / 60;
    let s = secs % 60;
    format!("{m:02}:{s:02}")
}

#[cfg(test)]
#[path = "../../../tests/app/handlers.rs"]
mod tests;

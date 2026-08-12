use super::*;
use ratatui::layout::{Constraint, Direction, Layout, Rect};

#[test]
fn default_theme_has_correct_colors() {
    let t = Theme::default();
    // Neutral-dark palette
    assert_eq!(t.border_active, Color::Rgb(0xd0, 0xd0, 0xd0));
    assert_eq!(t.border_inactive, Color::Rgb(0x77, 0x77, 0x77));
    assert_eq!(t.highlight_bg, Color::Rgb(0x2a, 0x2a, 0x2a));
    assert_eq!(t.text_primary, Color::Rgb(0xe6, 0xe6, 0xe6));
    assert_eq!(t.accent_color, Color::Rgb(0xc4, 0xc4, 0xc4));
    assert_eq!(t.background, Color::Rgb(0x14, 0x14, 0x14));
    assert_eq!(t.text_secondary, Color::Rgb(0x9e, 0x9e, 0x9e));
    assert_eq!(t.status_bar, Color::Rgb(0x1c, 0x1c, 0x1c));
    assert!(!t.show_ascii_art);
    // New semantic tokens
    assert_eq!(t.background_panel, Color::Rgb(0x1b, 0x1b, 0x1b));
    assert_eq!(t.background_element, Color::Rgb(0x24, 0x24, 0x24));
    assert_eq!(t.border_subtle, Color::Rgb(0x5a, 0x5a, 0x5a));
    assert_eq!(t.border_dimmest, Color::Rgb(0x30, 0x30, 0x30));
    assert_eq!(t.primary, Color::Rgb(0xd8, 0xd8, 0xd8));
    assert_eq!(t.success, Color::Rgb(0xc3, 0xe8, 0x8d));
    assert_eq!(t.error, Color::Rgb(0xff, 0x75, 0x7f));
    assert_eq!(t.warning, Color::Rgb(0xff, 0x96, 0x6c));
    assert_eq!(t.info, Color::Rgb(0xb8, 0xb8, 0xb8));
}

#[test]
fn default_compact_layout_structure() {
    let layout = default_compact_layout();
    assert_eq!(layout.direction, Some(SerializableDirection::Vertical));
    let constraints = layout.constraints.as_ref().unwrap();
    assert_eq!(constraints.len(), 3);
    assert_eq!(constraints[0], SerializableConstraint::Length(1));
    assert_eq!(constraints[1], SerializableConstraint::Fill(1));
    assert_eq!(constraints[2], SerializableConstraint::Length(1));

    let children = layout.children.as_ref().unwrap();
    assert_eq!(children.len(), 3);
    assert_eq!(children[0].widget, Some(UiWidget::Header));
    assert!(children[1].children.is_some());
    assert!(children[2].children.is_some());
}

#[test]
fn default_fullscreen_layout_structure() {
    let layout = default_fullscreen_layout();
    assert_eq!(layout.direction, Some(SerializableDirection::Vertical));
    let constraints = layout.constraints.as_ref().unwrap();
    assert_eq!(constraints.len(), 3);
    assert_eq!(constraints[0], SerializableConstraint::Length(18));
    assert_eq!(constraints[1], SerializableConstraint::Length(8));
    assert_eq!(constraints[2], SerializableConstraint::Min(0));

    let children = layout.children.as_ref().unwrap();
    assert_eq!(children.len(), 3);
    assert_eq!(children[0].widget, Some(UiWidget::NowPlaying));
    assert_eq!(children[1].widget, Some(UiWidget::FullscreenLyrics));
    assert_eq!(children[2].widget, Some(UiWidget::Visualizer));
}

#[test]
fn default_layout_tree_is_valid() {
    let layout = LayoutNode::default();
    assert_eq!(layout.direction, Some(SerializableDirection::Vertical));
    let constraints = layout.constraints.as_ref().unwrap();
    assert_eq!(constraints.len(), 4);
    assert_eq!(constraints[0], SerializableConstraint::Length(3));

    let children = layout.children.as_ref().unwrap();
    assert_eq!(children.len(), 4);
    assert_eq!(children[0].widget, Some(UiWidget::Header));
    assert_eq!(children[3].widget, Some(UiWidget::Help));
}

#[test]
fn serializable_direction_roundtrip() {
    let d: Direction = SerializableDirection::Horizontal.into();
    assert_eq!(d, Direction::Horizontal);
    let d: Direction = SerializableDirection::Vertical.into();
    assert_eq!(d, Direction::Vertical);
}

#[test]
fn serializable_constraint_roundtrip() {
    let pairs = [
        (SerializableConstraint::Length(10), Constraint::Length(10)),
        (
            SerializableConstraint::Percentage(50),
            Constraint::Percentage(50),
        ),
        (SerializableConstraint::Ratio(1, 3), Constraint::Ratio(1, 3)),
        (SerializableConstraint::Min(5), Constraint::Min(5)),
        (SerializableConstraint::Max(100), Constraint::Max(100)),
        (SerializableConstraint::Fill(2), Constraint::Fill(2)),
    ];
    for (sc, expected) in &pairs {
        let result: Constraint = (*sc).into();
        assert_eq!(result, *expected, "Mismatch for {:?}", sc);
    }
}

#[test]
fn parse_color_named() {
    assert_eq!(parse_color_from_str("red").unwrap(), Color::Red);
    assert_eq!(parse_color_from_str("green").unwrap(), Color::Green);
    assert_eq!(parse_color_from_str("blue").unwrap(), Color::Blue);
    assert_eq!(parse_color_from_str("black").unwrap(), Color::Black);
    assert_eq!(parse_color_from_str("white").unwrap(), Color::White);
    assert_eq!(parse_color_from_str("gray").unwrap(), Color::Gray);
    assert_eq!(parse_color_from_str("dark_gray").unwrap(), Color::DarkGray);
    assert_eq!(parse_color_from_str("transparent").unwrap(), Color::Reset);
    assert_eq!(parse_color_from_str("none").unwrap(), Color::Reset);
}

#[test]
fn parse_color_hex() {
    let c = parse_color_from_str("#ff0000").unwrap();
    assert_eq!(c, Color::Rgb(255, 0, 0));
    let c = parse_color_from_str("#00ff00").unwrap();
    assert_eq!(c, Color::Rgb(0, 255, 0));
    let c = parse_color_from_str("#0000ff").unwrap();
    assert_eq!(c, Color::Rgb(0, 0, 255));
    let c = parse_color_from_str("#abcdef").unwrap();
    assert_eq!(c, Color::Rgb(0xab, 0xcd, 0xef));
}

#[test]
fn parse_color_rgb_function() {
    let c = parse_color_from_str("rgb(10, 20, 30)").unwrap();
    assert_eq!(c, Color::Rgb(10, 20, 30));
    let c = parse_color_from_str("rgb(255,0,128)").unwrap();
    assert_eq!(c, Color::Rgb(255, 0, 128));
}

#[test]
fn parse_color_invalid_returns_err() {
    assert!(parse_color_from_str("notacolor").is_err());
    assert!(parse_color_from_str("").is_err());
    assert!(parse_color_from_str("#fff").is_err());
    assert!(parse_color_from_str("rgb(1)").is_err());
}

#[test]
fn theme_toml_roundtrip() {
    let t = Theme::default();
    let toml_str = toml::to_string_pretty(&t).unwrap();
    let deserialized: Theme = toml::from_str(&toml_str).unwrap();
    assert_eq!(t.border_active, deserialized.border_active);
    assert_eq!(t.border_inactive, deserialized.border_inactive);
    assert_eq!(t.background, deserialized.background);
    assert_eq!(t.text_secondary, deserialized.text_secondary);
    assert_eq!(
        t.fullscreen_layout.children.as_ref().unwrap().len(),
        deserialized
            .fullscreen_layout
            .children
            .as_ref()
            .unwrap()
            .len()
    );
}

#[test]
fn widget_style_to_style() {
    let ws = WidgetStyle {
        fg: Some(Color::Red),
        bg: Some(Color::Black),
        bold: true,
        italic: false,
    };
    let s: Style = ws.into();
    let _ = s;
}

#[test]
fn load_ascii_art_inline() {
    let theme = Theme {
        ascii_art_inline: Some(vec!["line1".into(), "line2".into()]),
        ..Default::default()
    };
    let art = theme.load_ascii_art();
    assert!(art.is_some());
    assert_eq!(art.unwrap(), vec!["line1", "line2"]);
}

#[test]
fn load_ascii_art_empty_inline_returns_none() {
    let theme = Theme {
        ascii_art_inline: Some(vec![]),
        ..Default::default()
    };
    assert!(theme.load_ascii_art().is_none());
}

#[test]
fn color_to_string_roundtrip() {
    for color in &[
        Color::Black,
        Color::Red,
        Color::Green,
        Color::Blue,
        Color::White,
        Color::Gray,
        Color::DarkGray,
        Color::Rgb(12, 34, 56),
    ] {
        let s = color_to_string(color);
        let parsed = parse_color_from_str(&s).unwrap();
        assert_eq!(&parsed, color, "Mismatch for {:?}", color);
    }
}

#[test]
fn theme_default_show_ascii_art() {
    assert!(default_true());
}

fn print_node(node: &LayoutNode, depth: usize) {
    let indent = "  ".repeat(depth);
    println!(
        "{}node: widget={:?} dir={:?} constraints={:?} children={}",
        indent,
        node.widget,
        node.direction,
        node.constraints.as_ref().map(|v| v.len()).unwrap_or(0),
        node.children.as_ref().map(|v| v.len()).unwrap_or(0)
    );
    if let Some(children) = &node.children {
        for child in children {
            print_node(child, depth + 1);
        }
    }
}

fn collect_areas(node: &LayoutNode, area: Rect, areas: &mut Vec<(Option<UiWidget>, Rect)>) {
    if let Some(widget) = &node.widget {
        areas.push((Some(widget.clone()), area));
        return;
    }
    if let (Some(dir), Some(raw_constraints), Some(children)) =
        (node.direction, &node.constraints, &node.children)
    {
        if children.is_empty() {
            return;
        }
        let parsed: Vec<Constraint> = raw_constraints.iter().map(|&c| c.into()).collect();
        let chunks = Layout::default()
            .direction(Direction::from(dir))
            .constraints(parsed)
            .split(area);
        for (i, child) in children.iter().enumerate() {
            if let Some(chunk) = chunks.get(i) {
                collect_areas(child, *chunk, areas);
            }
        }
    }
}

fn rects_overlap(a: &Rect, b: &Rect) -> bool {
    a.x < b.x + b.width && a.x + a.width > b.x && a.y < b.y + b.height && a.y + a.height > b.y
}

#[test]
fn parse_user_theme_toml_nested_arrays() {
    let toml_str = r##"
border_active = "#8ec07c"
border_inactive = "#504945"
highlight_bg = "#3c3836"
text_primary = "#ebdbb2"
accent_color = "#fe8019"
background = "#282828"
text_secondary = "#a89984"
status_bar = "#1d2021"
show_ascii_art = true

[layout_tree]
direction = "vertical"
[[layout_tree.constraints]]
length = 3
[[layout_tree.constraints]]
fill = 1
[[layout_tree.constraints]]
length = 1
[[layout_tree.constraints]]
length = 1
[[layout_tree.children]]
widget = "header"
[[layout_tree.children]]
direction = "horizontal"
[[layout_tree.children.constraints]]
percentage = 12
[[layout_tree.children.constraints]]
fill = 1
[[layout_tree.children.children]]
direction = "vertical"
[[layout_tree.children.children.constraints]]
length = 7
[[layout_tree.children.children.constraints]]
fill = 1
[[layout_tree.children.children.children]]
widget = "library"
[[layout_tree.children.children.children]]
direction = "vertical"
[[layout_tree.children.children.children.constraints]]
fill = 2
[[layout_tree.children.children.children.constraints]]
fill = 1
[[layout_tree.children.children.children.children]]
widget = "playlists"
[[layout_tree.children.children.children.children]]
widget = "ascii_art"
[[layout_tree.children.children]]
direction = "vertical"
[[layout_tree.children.children.constraints]]
fill = 1
[[layout_tree.children.children.children]]
direction = "horizontal"
[[layout_tree.children.children.children.constraints]]
fill = 6
[[layout_tree.children.children.children.constraints]]
fill = 1
[[layout_tree.children.children.children.children]]
widget = "main_content"
[[layout_tree.children.children.children.children]]
direction = "vertical"
[[layout_tree.children.children.children.children.constraints]]
fill = 6
[[layout_tree.children.children.children.children.constraints]]
fill = 1
[[layout_tree.children.children.children.children.children]]
widget = "queue"
[[layout_tree.children.children.children.children.children]]
widget = "lyrics"
[[layout_tree.children]]
direction = "horizontal"
[[layout_tree.children.constraints]]
percentage = 30
[[layout_tree.children.constraints]]
fill = 1
[[layout_tree.children.children]]
widget = "marquee"
[[layout_tree.children.children]]
widget = "progress"
[[layout_tree.children]]
widget = "help"
    "##;

    let result: Result<Theme, _> = toml::from_str(toml_str);
    match &result {
        Ok(theme) => {
            println!("PARSE OK");
            let lt = &theme.layout_tree;
            println!("direction: {:?}", lt.direction);
            println!("constraints: {:?}", lt.constraints);
            println!(
                "children count: {:?}",
                lt.children.as_ref().map(|c| c.len())
            );
            print_node(lt, 0);
        }
        Err(e) => {
            println!("PARSE ERROR: {}", e);
        }
    }
    let theme = result.expect("Failed to parse user theme TOML");

    let lt = &theme.layout_tree;
    assert_eq!(lt.direction, Some(SerializableDirection::Vertical));
    let constraints = lt.constraints.as_ref().unwrap();
    assert_eq!(constraints.len(), 4);
    assert_eq!(constraints[0], SerializableConstraint::Length(3));
    assert_eq!(constraints[1], SerializableConstraint::Fill(1));
    assert_eq!(constraints[2], SerializableConstraint::Length(1));
    assert_eq!(constraints[3], SerializableConstraint::Length(1));

    let children = lt.children.as_ref().unwrap();
    assert_eq!(children.len(), 4);

    assert_eq!(children[0].widget, Some(UiWidget::Header));
    assert_eq!(children[3].widget, Some(UiWidget::Help));

    let middle = &children[1];
    assert_eq!(middle.direction, Some(SerializableDirection::Horizontal));
    let middle_constraints = middle.constraints.as_ref().unwrap();
    assert_eq!(middle_constraints.len(), 2);
    assert_eq!(
        middle_constraints[0],
        SerializableConstraint::Percentage(12)
    );
    assert_eq!(middle_constraints[1], SerializableConstraint::Fill(1));

    let middle_children = middle.children.as_ref().unwrap();
    assert_eq!(middle_children.len(), 2);

    let sidebar = &middle_children[0];
    assert_eq!(sidebar.direction, Some(SerializableDirection::Vertical));
    let sidebar_constraints = sidebar.constraints.as_ref().unwrap();
    assert_eq!(sidebar_constraints.len(), 2);
    assert_eq!(sidebar_constraints[0], SerializableConstraint::Length(7));
    let sidebar_children = sidebar.children.as_ref().unwrap();
    assert_eq!(sidebar_children.len(), 2);
    assert_eq!(sidebar_children[0].widget, Some(UiWidget::Library));

    let content = &middle_children[1];
    assert_eq!(content.direction, Some(SerializableDirection::Vertical));
    let content_children = content.children.as_ref().unwrap();
    assert_eq!(content_children.len(), 1);
    let content_horizontal = &content_children[0];
    assert_eq!(
        content_horizontal.direction,
        Some(SerializableDirection::Horizontal)
    );
    let ch_children = content_horizontal.children.as_ref().unwrap();
    assert_eq!(ch_children.len(), 2);
    assert_eq!(ch_children[0].widget, Some(UiWidget::MainContent));
}

#[test]
fn user_theme_layout_areas_do_not_overlap() {
    let toml_str = r##"
border_active = "#8ec07c"
border_inactive = "#504945"
highlight_bg = "#3c3836"
text_primary = "#ebdbb2"
accent_color = "#fe8019"
background = "#282828"
text_secondary = "#a89984"
status_bar = "#1d2021"
show_ascii_art = true

[layout_tree]
direction = "vertical"
[[layout_tree.constraints]]
length = 3
[[layout_tree.constraints]]
fill = 1
[[layout_tree.constraints]]
length = 1
[[layout_tree.constraints]]
length = 1
[[layout_tree.children]]
widget = "header"
[[layout_tree.children]]
direction = "horizontal"
[[layout_tree.children.constraints]]
percentage = 12
[[layout_tree.children.constraints]]
fill = 1
[[layout_tree.children.children]]
direction = "vertical"
[[layout_tree.children.children.constraints]]
length = 7
[[layout_tree.children.children.constraints]]
fill = 1
[[layout_tree.children.children.children]]
widget = "library"
[[layout_tree.children.children.children]]
direction = "vertical"
[[layout_tree.children.children.children.constraints]]
fill = 2
[[layout_tree.children.children.children.constraints]]
fill = 1
[[layout_tree.children.children.children.children]]
widget = "playlists"
[[layout_tree.children.children.children.children]]
widget = "ascii_art"
[[layout_tree.children.children]]
direction = "vertical"
[[layout_tree.children.children.constraints]]
fill = 1
[[layout_tree.children.children.children]]
direction = "horizontal"
[[layout_tree.children.children.children.constraints]]
fill = 6
[[layout_tree.children.children.children.constraints]]
fill = 1
[[layout_tree.children.children.children.children]]
widget = "main_content"
[[layout_tree.children.children.children.children]]
direction = "vertical"
[[layout_tree.children.children.children.children.constraints]]
fill = 6
[[layout_tree.children.children.children.children.constraints]]
fill = 1
[[layout_tree.children.children.children.children.children]]
widget = "queue"
[[layout_tree.children.children.children.children.children]]
widget = "lyrics"
[[layout_tree.children]]
direction = "horizontal"
[[layout_tree.children.constraints]]
percentage = 30
[[layout_tree.children.constraints]]
fill = 1
[[layout_tree.children.children]]
widget = "marquee"
[[layout_tree.children.children]]
widget = "progress"
[[layout_tree.children]]
widget = "help"
    "##;

    let theme: Theme = toml::from_str(toml_str).unwrap();

    let test_area = Rect::new(1, 1, 120, 38);
    let mut areas: Vec<(Option<UiWidget>, Rect)> = Vec::new();
    collect_areas(&theme.layout_tree, test_area, &mut areas);

    println!(
        "Widget areas for {}x{} terminal:",
        test_area.width, test_area.height
    );
    for (widget, area) in &areas {
        println!(
            "  {:?}: ({}, {}, {}, {})",
            widget, area.x, area.y, area.width, area.height
        );
    }

    for i in 0..areas.len() {
        for j in (i + 1)..areas.len() {
            if rects_overlap(&areas[i].1, &areas[j].1) {
                panic!(
                    "OVERLAP: {:?} at ({},{},{},{}) overlaps with {:?} at ({},{},{},{})",
                    areas[i].0,
                    areas[i].1.x,
                    areas[i].1.y,
                    areas[i].1.width,
                    areas[i].1.height,
                    areas[j].0,
                    areas[j].1.x,
                    areas[j].1.y,
                    areas[j].1.width,
                    areas[j].1.height
                );
            }
        }
    }
}

#[test]
fn default_highlight_symbols() {
    let t = Theme::default();
    assert_eq!(t.highlight_symbol, "> ");
    assert_eq!(t.options_panel_symbol, "▶ ");
}

#[test]
fn highlight_symbols_roundtrip() {
    let toml_str = r#"
border_active = "red"
border_inactive = "gray"
highlight_bg = "rgb(40,40,40)"
text_primary = "white"
accent_color = "green"
background = "rgb(20,20,20)"
text_secondary = "gray"
status_bar = "rgb(30,30,30)"
highlight_symbol = "→ "
options_panel_symbol = "◆ "
"#;
    let theme: Theme = toml::from_str(toml_str).unwrap();
    assert_eq!(theme.highlight_symbol, "→ ");
    assert_eq!(theme.options_panel_symbol, "◆ ");
}

#[test]
fn highlight_symbols_omit_uses_defaults() {
    let toml_str = r#"
border_active = "red"
border_inactive = "gray"
highlight_bg = "rgb(40,40,40)"
text_primary = "white"
accent_color = "green"
background = "rgb(20,20,20)"
text_secondary = "gray"
status_bar = "rgb(30,30,30)"
"#;
    let theme: Theme = toml::from_str(toml_str).unwrap();
    assert_eq!(theme.highlight_symbol, "> ");
    assert_eq!(theme.options_panel_symbol, "▶ ");
}

// --- Reactive theme & visualizer/border config tests ---

#[test]
fn theme_lerp_endpoints() {
    let a = Theme::default();
    let mut b = Theme::default();
    b.primary = Color::Rgb(255, 0, 0);
    b.accent_color = Color::Rgb(0, 255, 0);

    // t=0 should equal `from` on color fields.
    let at_zero = Theme::lerp(&a, &b, 0.0);
    assert_eq!(at_zero.primary, a.primary);
    assert_eq!(at_zero.accent_color, a.accent_color);

    // t=1 should equal `to`.
    let at_one = Theme::lerp(&a, &b, 1.0);
    assert_eq!(at_one.primary, b.primary);
    assert_eq!(at_one.accent_color, b.accent_color);
}

#[test]
fn theme_lerp_midpoint() {
    let a = Theme::default();
    let mut b = Theme::default();
    b.primary = Color::Rgb(0, 0, 100);
    // a.primary is Rgb(0xd8, 0xd8, 0xd8) = (216, 216, 216)
    // midpoint with (0, 0, 100) = (108, 108, 158)
    let mid = Theme::lerp(&a, &b, 0.5);
    if let Color::Rgb(r, g, bl) = mid.primary {
        assert!((i16::from(r) - 108).abs() <= 1, "r={r}");
        assert!((i16::from(g) - 108).abs() <= 1, "g={g}");
        assert!((i16::from(bl) - 158).abs() <= 1, "b={bl}");
    } else {
        panic!("expected Rgb");
    }
}

#[test]
fn reactive_theme_defaults_off() {
    let t = Theme::default();
    assert!(!t.reactive_theme);
    assert_eq!(t.reactive_cross_fade_ms, 800);
}

#[test]
fn reactive_theme_roundtrip() {
    let toml_str = r#"
border_active = "red"
border_inactive = "gray"
highlight_bg = "rgb(40,40,40)"
text_primary = "white"
accent_color = "green"
background = "rgb(20,20,20)"
text_secondary = "gray"
status_bar = "rgb(30,30,30)"
reactive_theme = true
reactive_cross_fade_ms = 1200
"#;
    let theme: Theme = toml::from_str(toml_str).unwrap();
    assert!(theme.reactive_theme);
    assert_eq!(theme.reactive_cross_fade_ms, 1200);
}

#[test]
fn visualizer_config_defaults() {
    let t = Theme::default();
    assert_eq!(t.visualizer.style, VisualizerStyle::BrailleBars);
    assert!(t.visualizer.color.is_none());
    assert!(t.visualizer.bar_count.is_none());
}

#[test]
fn visualizer_config_roundtrip() {
    let toml_str = r##"
border_active = "red"
border_inactive = "gray"
highlight_bg = "rgb(40,40,40)"
text_primary = "white"
accent_color = "green"
background = "rgb(20,20,20)"
text_secondary = "gray"
status_bar = "rgb(30,30,30)"

[visualizer]
style = "plasma"
color = "#ff8800"
bar_count = 32
height = 8
"##;
    let theme: Theme = toml::from_str(toml_str).unwrap();
    assert_eq!(theme.visualizer.style, VisualizerStyle::Plasma);
    assert_eq!(theme.visualizer.color, Some(Color::Rgb(0xff, 0x88, 0x00)));
    assert_eq!(theme.visualizer.bar_count, Some(32));
    assert_eq!(theme.visualizer.height, Some(8));
}

#[test]
fn visualizer_block_bars_roundtrip() {
    let toml_str = r##"
border_active = "red"
border_inactive = "gray"
highlight_bg = "rgb(40,40,40)"
text_primary = "white"
accent_color = "green"
background = "rgb(20,20,20)"
text_secondary = "gray"
status_bar = "rgb(30,30,30)"

[visualizer]
style = "block_bars"
"##;
    let theme: Theme = toml::from_str(toml_str).unwrap();
    assert_eq!(theme.visualizer.style, VisualizerStyle::BlockBars);
}

#[test]
fn border_config_roundtrip() {
    let toml_str = r##"
border_active = "red"
border_inactive = "gray"
highlight_bg = "rgb(40,40,40)"
text_primary = "white"
accent_color = "green"
background = "rgb(20,20,20)"
text_secondary = "gray"
status_bar = "rgb(30,30,30)"

[borders.library]
style = "rounded"
color_focused = "#ff0000"
color_unfocused = "#444444"

[borders.queue]
style = "none"
"##;
    let theme: Theme = toml::from_str(toml_str).unwrap();
    let lib = theme.borders.get(&UiWidget::Library).unwrap();
    assert_eq!(lib.style, BorderStyle::Rounded);
    assert_eq!(lib.color_focused, Some(Color::Rgb(0xff, 0, 0)));
    assert_eq!(lib.color_unfocused, Some(Color::Rgb(0x44, 0x44, 0x44)));

    let q = theme.borders.get(&UiWidget::Queue).unwrap();
    assert_eq!(q.style, BorderStyle::None);
    assert!(q.color_focused.is_none());
}

#[test]
fn focus_layout_toml_roundtrip() {
    let mut t = Theme::default();
    t.show_ascii_art = false;
    t.layout_tree = LayoutNode {
        direction: Some(SerializableDirection::Vertical),
        constraints: Some(vec![
            SerializableConstraint::Fill(1),
            SerializableConstraint::Length(1),
        ]),
        widget: None,
        children: Some(vec![
            LayoutNode {
                direction: Some(SerializableDirection::Horizontal),
                constraints: Some(vec![
                    SerializableConstraint::Percentage(30),
                    SerializableConstraint::Fill(1),
                ]),
                widget: None,
                children: Some(vec![
                    LayoutNode {
                        widget: Some(UiWidget::AlbumArt),
                        direction: None,
                        constraints: None,
                        children: None,
                    },
                    LayoutNode {
                        widget: Some(UiWidget::MainContent),
                        direction: None,
                        constraints: None,
                        children: None,
                    },
                ]),
            },
            LayoutNode {
                direction: Some(SerializableDirection::Horizontal),
                constraints: Some(vec![
                    SerializableConstraint::Percentage(30),
                    SerializableConstraint::Fill(1),
                ]),
                widget: None,
                children: Some(vec![
                    LayoutNode {
                        widget: Some(UiWidget::Marquee),
                        direction: None,
                        constraints: None,
                        children: None,
                    },
                    LayoutNode {
                        widget: Some(UiWidget::Progress),
                        direction: None,
                        constraints: None,
                        children: None,
                    },
                ]),
            },
        ]),
    };

    let toml_str = toml::to_string_pretty(&t).unwrap();
    println!("Serialized Focus layout:\n{}", toml_str);

    let deserialized: Theme = toml::from_str(&toml_str).unwrap();

    // Verify top-level structure
    assert_eq!(
        deserialized.layout_tree.direction,
        Some(SerializableDirection::Vertical)
    );
    let constraints = deserialized.layout_tree.constraints.as_ref().unwrap();
    assert_eq!(constraints.len(), 2);
    assert_eq!(constraints[0], SerializableConstraint::Fill(1));
    assert_eq!(constraints[1], SerializableConstraint::Length(1));

    let children = deserialized.layout_tree.children.as_ref().unwrap();
    assert_eq!(children.len(), 2);

    // First child: horizontal split with AlbumArt + MainContent
    let top = &children[0];
    assert_eq!(top.direction, Some(SerializableDirection::Horizontal));
    let top_children = top.children.as_ref().unwrap();
    assert_eq!(top_children.len(), 2);
    assert_eq!(top_children[0].widget, Some(UiWidget::AlbumArt));
    assert_eq!(top_children[1].widget, Some(UiWidget::MainContent));

    // Second child: horizontal split with Marquee + Progress
    let bottom = &children[1];
    assert_eq!(bottom.direction, Some(SerializableDirection::Horizontal));
    let bottom_children = bottom.children.as_ref().unwrap();
    assert_eq!(bottom_children.len(), 2);
    assert_eq!(bottom_children[0].widget, Some(UiWidget::Marquee));
    assert_eq!(bottom_children[1].widget, Some(UiWidget::Progress));

    // Verify no overlaps in rendered areas
    let test_area = Rect::new(1, 1, 120, 38);
    let mut areas: Vec<(Option<UiWidget>, Rect)> = Vec::new();
    collect_areas(&deserialized.layout_tree, test_area, &mut areas);
    for i in 0..areas.len() {
        for j in (i + 1)..areas.len() {
            if rects_overlap(&areas[i].1, &areas[j].1) {
                panic!(
                    "OVERLAP: {:?} at {:?} overlaps with {:?} at {:?}",
                    areas[i].0, areas[i].1, areas[j].0, areas[j].1
                );
            }
        }
    }
}

#[test]
fn border_config_defaults_to_left_bar() {
    let cfg = BorderConfig::default();
    assert_eq!(cfg.style, BorderStyle::LeftBar);
    assert!(cfg.color_focused.is_none());
    assert!(cfg.color_unfocused.is_none());
}

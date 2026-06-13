use super::*;
use ratatui::layout::Constraint;

#[test]
fn default_theme_has_correct_colors() {
    let t = Theme::default();
    assert_eq!(t.border_active, Color::Green);
    assert_eq!(t.border_inactive, Color::DarkGray);
    assert_eq!(t.highlight_bg, Color::Rgb(40, 40, 40));
    assert_eq!(t.text_primary, Color::White);
    assert_eq!(t.accent_color, Color::Green);
    assert_eq!(t.background, Color::Rgb(20, 20, 20));
    assert_eq!(t.text_secondary, Color::Gray);
    assert_eq!(t.status_bar, Color::Rgb(30, 30, 30));
    assert!(t.show_ascii_art);
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

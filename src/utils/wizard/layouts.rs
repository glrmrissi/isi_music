use console::{Term, style};
use dialoguer::Select;

use crate::utils::theme::{
    BorderConfig, BorderStyle, LayoutNode, SerializableConstraint, SerializableDirection, Theme,
    UiWidget, VisualizerStyle,
};

use super::helpers::{header, theme as dialog_theme};
use super::parse_hex;

pub(super) struct LayoutPreset {
    pub name: &'static str,
    pub diagram: &'static str,
    pub build: fn(&mut Theme),
}

pub(super) const LAYOUTS: &[LayoutPreset] = &[
    LayoutPreset {
        name: "Default",
        diagram: "  ┌─Header──┬─Help──┐\n  │ Lib │ Main │ Q │\n  │  PL │      │   │\n  │ Marq│ Prog │   │\n  └─────┴──────┴───┘",
        build: apply_default_layout,
    },
    LayoutPreset {
        name: "Clean",
        diagram: "  ┌──────── Search ────────┐\n  ├──────┬───────────┤\n  │ Lib  │           │\n  │      │  Main     │\n  │  PL  │           │\n  ├──────┴───────────┤\n  │ Marq │  Progress │\n  └──────┴───────────┘",
        build: apply_clean_layout,
    },
    LayoutPreset {
        name: "Focus",
        diagram: "  ┌──────── Search ────────┐\n  ├───────┬───────────┤\n  │ Album │           │\n  │  Art  │   Main    │\n  ├───────┤           │\n  │ Lib   │           │\n  ├───────┤           │\n  │  PL   │           │\n  ├───────┴───────────┤\n  │ Marq │  Progress │\n  └──────┴───────────┘",
        build: apply_focus_layout,
    },
    LayoutPreset {
        name: "Sidebar Right",
        diagram: "  ┌─Header────────────┐\n  │           │ Lib   │\n  │   Main    │  PL   │\n  │           │ Queue │\n  ├───────────┴───────┤\n  │ Marq │  Progress  │\n  └─────┴─────────────┘",
        build: apply_sidebar_right_layout,
    },
    LayoutPreset {
        name: "Full Player",
        diagram: "  ┌──────── Search ────────┐\n  ├───────┬───────────┤\n  │ Album │           │\n  │  Art  │   Main    │\n  ├───────┤           │\n  │ Marq  │           │\n  ├───────┼───────────┤\n  │ Lib   │           │\n  ├───────┤  Progress │\n  │  PL   │           │\n  └───────┴───────────┘",
        build: apply_full_player_layout,
    },
    LayoutPreset {
        name: "Showcase",
        diagram: "  ┌──────── Search ────────┐\n  ├──────┬───────┬──────┤\n  │ Lib  │ Album │ Main │\n  │      │  Art  │      │\n  │  PL  │ Artist│ Queue│\n  │      ├───────┤      │\n  │      │ Lyrics│      │\n  │      ├───────┤      │\n  │      │  Viz  │      │\n  ├──────┴───────┴──────┤\n  │ Marq │   Progress   │\n  └──────┴──────────────┘",
        build: apply_showcase_layout,
    },
];

pub fn pick_layout(term: &Term) -> anyhow::Result<&'static str> {
    header(term, "— Layout");

    println!("  Choose how widgets are arranged on screen.\n");
    println!(
        "  {}\n",
        style("Every widget can go anywhere — these are just starting points.").dim()
    );
    println!("  You can fine-tune the layout in theme.toml afterwards.\n");

    let items: Vec<String> = LAYOUTS
        .iter()
        .map(|l| format!("{:<16} {}", l.name, l.diagram.lines().next().unwrap_or("")))
        .collect();

    let idx = Select::with_theme(&dialog_theme())
        .with_prompt("Layout preset")
        .items(&items)
        .default(0)
        .interact()?;

    let layout = &LAYOUTS[idx];

    println!();
    for line in layout.diagram.lines() {
        println!("  {}", style(line).cyan());
    }
    println!();

    println!(
        "  {} {}",
        style("[OK]").green(),
        style(format!("Layout: {}", layout.name)).bold()
    );

    Ok(layout.name)
}

pub fn apply_layout_to_theme(t: &mut Theme, chosen: &str) {
    for l in LAYOUTS {
        if l.name == chosen {
            (l.build)(t);
            return;
        }
    }
}

fn apply_default_layout(_t: &mut Theme) {
    let default = Theme::default();
    _t.layout_tree = default.layout_tree;
    _t.compact_layout = default.compact_layout;
    _t.borders = default.borders;
    _t.show_ascii_art = true;
}

fn apply_clean_layout(t: &mut Theme) {
    use std::collections::HashMap;

    t.show_ascii_art = false;

    let mut borders = HashMap::new();
    let left_bar = |focused: &str| BorderConfig {
        style: BorderStyle::LeftBar,
        color_focused: Some(parse_hex(focused)),
        color_unfocused: Some(parse_hex("#3b3d4f")),
    };
    let no_border = || BorderConfig {
        style: BorderStyle::None,
        color_focused: None,
        color_unfocused: None,
    };

    borders.insert(UiWidget::MainContent, left_bar("#89b4fa"));
    borders.insert(UiWidget::Library, left_bar("#89b4fa"));
    borders.insert(UiWidget::Playlists, left_bar("#89b4fa"));
    borders.insert(UiWidget::Queue, no_border());
    borders.insert(UiWidget::Header, no_border());
    borders.insert(UiWidget::Progress, no_border());
    borders.insert(UiWidget::Marquee, no_border());
    borders.insert(UiWidget::Help, no_border());
    t.borders = borders;

    let sidebar = LayoutNode {
        direction: Some(SerializableDirection::Vertical),
        constraints: Some(vec![
            SerializableConstraint::Length(7),
            SerializableConstraint::Fill(1),
        ]),
        widget: None,
        children: Some(vec![leaf(UiWidget::Library), leaf(UiWidget::Playlists)]),
    };

    t.layout_tree = LayoutNode {
        direction: Some(SerializableDirection::Vertical),
        constraints: Some(vec![
            SerializableConstraint::Length(1),
            SerializableConstraint::Fill(1),
            SerializableConstraint::Length(1),
        ]),
        widget: None,
        children: Some(vec![
            leaf(UiWidget::Search),
            LayoutNode {
                direction: Some(SerializableDirection::Horizontal),
                constraints: Some(vec![
                    SerializableConstraint::Percentage(22),
                    SerializableConstraint::Fill(1),
                ]),
                widget: None,
                children: Some(vec![sidebar, leaf(UiWidget::MainContent)]),
            },
            bottom_bar(),
        ]),
    };

    t.compact_layout = t.layout_tree.clone();
}

fn apply_focus_layout(t: &mut Theme) {
    use std::collections::HashMap;

    t.show_ascii_art = true;

    let mut borders = HashMap::new();
    let no_border = || BorderConfig {
        style: BorderStyle::None,
        color_focused: None,
        color_unfocused: None,
    };
    let rounded = |focused: &str| BorderConfig {
        style: BorderStyle::Rounded,
        color_focused: Some(parse_hex(focused)),
        color_unfocused: Some(parse_hex("#3b3d4f")),
    };
    let left_bar = |focused: &str| BorderConfig {
        style: BorderStyle::LeftBar,
        color_focused: Some(parse_hex(focused)),
        color_unfocused: Some(parse_hex("#3b3d4f")),
    };

    borders.insert(UiWidget::AlbumArt, rounded("#89b4fa"));
    borders.insert(UiWidget::MainContent, left_bar("#89b4fa"));
    borders.insert(UiWidget::Library, left_bar("#89b4fa"));
    borders.insert(UiWidget::Playlists, left_bar("#89b4fa"));
    borders.insert(UiWidget::Header, no_border());
    borders.insert(UiWidget::Queue, no_border());
    borders.insert(UiWidget::Progress, no_border());
    borders.insert(UiWidget::Marquee, no_border());
    borders.insert(UiWidget::Help, no_border());
    t.borders = borders;

    let left_col = LayoutNode {
        direction: Some(SerializableDirection::Vertical),
        constraints: Some(vec![
            SerializableConstraint::Length(10),
            SerializableConstraint::Fill(1),
            SerializableConstraint::Fill(1),
        ]),
        widget: None,
        children: Some(vec![
            leaf(UiWidget::AlbumArt),
            leaf(UiWidget::Library),
            leaf(UiWidget::Playlists),
        ]),
    };

    t.layout_tree = LayoutNode {
        direction: Some(SerializableDirection::Vertical),
        constraints: Some(vec![
            SerializableConstraint::Length(1),
            SerializableConstraint::Fill(1),
            SerializableConstraint::Length(1),
        ]),
        widget: None,
        children: Some(vec![
            leaf(UiWidget::Search),
            LayoutNode {
                direction: Some(SerializableDirection::Horizontal),
                constraints: Some(vec![
                    SerializableConstraint::Percentage(28),
                    SerializableConstraint::Fill(1),
                ]),
                widget: None,
                children: Some(vec![left_col, leaf(UiWidget::MainContent)]),
            },
            bottom_bar(),
        ]),
    };

    t.compact_layout = t.layout_tree.clone();
}

fn apply_sidebar_right_layout(t: &mut Theme) {
    use std::collections::HashMap;

    t.show_ascii_art = true;

    let mut borders = HashMap::new();
    let left_bar = |focused: &str| BorderConfig {
        style: BorderStyle::LeftBar,
        color_focused: Some(parse_hex(focused)),
        color_unfocused: Some(parse_hex("#3b3d4f")),
    };
    let no_border = || BorderConfig {
        style: BorderStyle::None,
        color_focused: None,
        color_unfocused: None,
    };

    borders.insert(UiWidget::MainContent, left_bar("#89b4fa"));
    borders.insert(UiWidget::Library, left_bar("#89b4fa"));
    borders.insert(UiWidget::Playlists, left_bar("#89b4fa"));
    borders.insert(UiWidget::Queue, left_bar("#89b4fa"));
    borders.insert(UiWidget::Header, no_border());
    borders.insert(UiWidget::Progress, no_border());
    borders.insert(UiWidget::Marquee, no_border());
    borders.insert(UiWidget::Help, no_border());
    t.borders = borders;

    let right_sidebar = LayoutNode {
        direction: Some(SerializableDirection::Vertical),
        constraints: Some(vec![
            SerializableConstraint::Length(7),
            SerializableConstraint::Fill(1),
            SerializableConstraint::Length(8),
        ]),
        widget: None,
        children: Some(vec![
            leaf(UiWidget::Library),
            leaf(UiWidget::Playlists),
            leaf(UiWidget::Queue),
        ]),
    };

    t.layout_tree = LayoutNode {
        direction: Some(SerializableDirection::Vertical),
        constraints: Some(vec![
            SerializableConstraint::Length(3),
            SerializableConstraint::Fill(1),
            SerializableConstraint::Length(1),
        ]),
        widget: None,
        children: Some(vec![
            leaf(UiWidget::Header),
            LayoutNode {
                direction: Some(SerializableDirection::Horizontal),
                constraints: Some(vec![
                    SerializableConstraint::Fill(1),
                    SerializableConstraint::Percentage(25),
                ]),
                widget: None,
                children: Some(vec![leaf(UiWidget::MainContent), right_sidebar]),
            },
            bottom_bar(),
        ]),
    };

    t.compact_layout = t.layout_tree.clone();
}

fn apply_full_player_layout(t: &mut Theme) {
    use std::collections::HashMap;

    t.show_ascii_art = true;

    let mut borders = HashMap::new();
    let no_border = || BorderConfig {
        style: BorderStyle::None,
        color_focused: None,
        color_unfocused: None,
    };
    let rounded = |focused: &str| BorderConfig {
        style: BorderStyle::Rounded,
        color_focused: Some(parse_hex(focused)),
        color_unfocused: Some(parse_hex("#3b3d4f")),
    };
    let left_bar = |focused: &str| BorderConfig {
        style: BorderStyle::LeftBar,
        color_focused: Some(parse_hex(focused)),
        color_unfocused: Some(parse_hex("#3b3d4f")),
    };

    borders.insert(UiWidget::AlbumArt, rounded("#89b4fa"));
    borders.insert(UiWidget::MainContent, left_bar("#89b4fa"));
    borders.insert(UiWidget::Library, left_bar("#89b4fa"));
    borders.insert(UiWidget::Playlists, left_bar("#89b4fa"));
    borders.insert(UiWidget::Header, no_border());
    borders.insert(UiWidget::Queue, no_border());
    borders.insert(UiWidget::Progress, no_border());
    borders.insert(UiWidget::Marquee, no_border());
    borders.insert(UiWidget::Help, no_border());
    t.borders = borders;

    let left_col = LayoutNode {
        direction: Some(SerializableDirection::Vertical),
        constraints: Some(vec![
            SerializableConstraint::Length(10),
            SerializableConstraint::Length(3),
            SerializableConstraint::Fill(1),
            SerializableConstraint::Fill(1),
        ]),
        widget: None,
        children: Some(vec![
            leaf(UiWidget::AlbumArt),
            leaf(UiWidget::Marquee),
            leaf(UiWidget::Library),
            leaf(UiWidget::Playlists),
        ]),
    };

    let right_col = LayoutNode {
        direction: Some(SerializableDirection::Vertical),
        constraints: Some(vec![
            SerializableConstraint::Fill(1),
            SerializableConstraint::Length(1),
        ]),
        widget: None,
        children: Some(vec![leaf(UiWidget::MainContent), leaf(UiWidget::Progress)]),
    };

    t.layout_tree = LayoutNode {
        direction: Some(SerializableDirection::Vertical),
        constraints: Some(vec![
            SerializableConstraint::Length(1),
            SerializableConstraint::Fill(1),
        ]),
        widget: None,
        children: Some(vec![
            leaf(UiWidget::Search),
            LayoutNode {
                direction: Some(SerializableDirection::Horizontal),
                constraints: Some(vec![
                    SerializableConstraint::Percentage(28),
                    SerializableConstraint::Fill(1),
                ]),
                widget: None,
                children: Some(vec![left_col, right_col]),
            },
        ]),
    };

    t.compact_layout = t.layout_tree.clone();
}

fn apply_showcase_layout(t: &mut Theme) {
    use std::collections::HashMap;

    t.show_ascii_art = false;
    t.visualizer.style = VisualizerStyle::BlockBars;

    let mut borders = HashMap::new();
    let left_bar = |focused: &str| BorderConfig {
        style: BorderStyle::LeftBar,
        color_focused: Some(parse_hex(focused)),
        color_unfocused: Some(parse_hex("#3b3d4f")),
    };
    let no_border = || BorderConfig {
        style: BorderStyle::None,
        color_focused: None,
        color_unfocused: None,
    };

    borders.insert(UiWidget::MainContent, left_bar("#89b4fa"));
    borders.insert(UiWidget::Library, left_bar("#89b4fa"));
    borders.insert(UiWidget::Playlists, left_bar("#89b4fa"));
    borders.insert(UiWidget::Queue, left_bar("#89b4fa"));
    borders.insert(UiWidget::AlbumArt, no_border());
    borders.insert(UiWidget::AlbumArtWithInfo, no_border());
    borders.insert(UiWidget::Visualizer, no_border());
    borders.insert(UiWidget::Header, no_border());
    borders.insert(UiWidget::Search, left_bar("#89b4fa"));
    borders.insert(UiWidget::Progress, no_border());
    borders.insert(UiWidget::Marquee, no_border());
    borders.insert(UiWidget::Help, no_border());
    t.borders = borders;

    let left_col = LayoutNode {
        direction: Some(SerializableDirection::Vertical),
        constraints: Some(vec![
            SerializableConstraint::Length(7),
            SerializableConstraint::Fill(1),
        ]),
        widget: None,
        children: Some(vec![leaf(UiWidget::Library), leaf(UiWidget::Playlists)]),
    };

    let center_col = LayoutNode {
        direction: Some(SerializableDirection::Vertical),
        constraints: Some(vec![
            SerializableConstraint::Percentage(65),
            SerializableConstraint::Length(3),
            SerializableConstraint::Fill(1),
        ]),
        widget: None,
        children: Some(vec![
            leaf(UiWidget::AlbumArtWithInfo),
            leaf(UiWidget::Lyrics),
            leaf(UiWidget::Visualizer),
        ]),
    };

    let right_col = LayoutNode {
        direction: Some(SerializableDirection::Vertical),
        constraints: Some(vec![
            SerializableConstraint::Fill(1),
            SerializableConstraint::Length(8),
        ]),
        widget: None,
        children: Some(vec![leaf(UiWidget::MainContent), leaf(UiWidget::Queue)]),
    };

    t.layout_tree = LayoutNode {
        direction: Some(SerializableDirection::Vertical),
        constraints: Some(vec![
            SerializableConstraint::Length(1),
            SerializableConstraint::Fill(1),
            SerializableConstraint::Length(1),
        ]),
        widget: None,
        children: Some(vec![
            leaf(UiWidget::Search),
            LayoutNode {
                direction: Some(SerializableDirection::Horizontal),
                constraints: Some(vec![
                    SerializableConstraint::Percentage(20),
                    SerializableConstraint::Fill(1),
                    SerializableConstraint::Percentage(28),
                ]),
                widget: None,
                children: Some(vec![left_col, center_col, right_col]),
            },
            bottom_bar(),
        ]),
    };

    t.compact_layout = t.layout_tree.clone();
}

fn leaf(widget: UiWidget) -> LayoutNode {
    LayoutNode {
        widget: Some(widget),
        direction: None,
        constraints: None,
        children: None,
    }
}

fn bottom_bar() -> LayoutNode {
    LayoutNode {
        direction: Some(SerializableDirection::Horizontal),
        constraints: Some(vec![
            SerializableConstraint::Percentage(30),
            SerializableConstraint::Fill(1),
        ]),
        widget: None,
        children: Some(vec![leaf(UiWidget::Marquee), leaf(UiWidget::Progress)]),
    }
}

use serde::{Deserialize, Serialize};

use super::serde_types::{SerializableConstraint, SerializableDirection, UiWidget};

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct LayoutNode {
    pub direction: Option<SerializableDirection>,
    pub constraints: Option<Vec<SerializableConstraint>>,
    pub children: Option<Vec<LayoutNode>>,
    pub widget: Option<UiWidget>,
}

impl Default for LayoutNode {
    fn default() -> Self {
        use SerializableConstraint::*;
        Self {
            direction: Some(SerializableDirection::Vertical),
            constraints: Some(vec![Length(3), Fill(1), Length(1), Length(1)]),
            widget: None,
            children: Some(vec![
                LayoutNode {
                    widget: Some(UiWidget::Header),
                    direction: None,
                    constraints: None,
                    children: None,
                },
                LayoutNode {
                    direction: Some(SerializableDirection::Horizontal),
                    constraints: Some(vec![Percentage(25), Fill(1)]),
                    widget: None,
                    children: Some(vec![
                        LayoutNode {
                            direction: Some(SerializableDirection::Vertical),
                            constraints: Some(vec![Length(7), Fill(1)]),
                            widget: None,
                            children: Some(vec![
                                LayoutNode {
                                    widget: Some(UiWidget::Library),
                                    direction: None,
                                    constraints: None,
                                    children: None,
                                },
                                LayoutNode {
                                    widget: Some(UiWidget::Playlists),
                                    direction: None,
                                    constraints: None,
                                    children: None,
                                },
                            ]),
                        },
                        LayoutNode {
                            direction: Some(SerializableDirection::Vertical),
                            constraints: Some(vec![Fill(1), Length(8)]),
                            widget: None,
                            children: Some(vec![
                                LayoutNode {
                                    widget: Some(UiWidget::MainContent),
                                    direction: None,
                                    constraints: None,
                                    children: None,
                                },
                                LayoutNode {
                                    widget: Some(UiWidget::Queue),
                                    direction: None,
                                    constraints: None,
                                    children: None,
                                },
                            ]),
                        },
                    ]),
                },
                LayoutNode {
                    direction: Some(SerializableDirection::Horizontal),
                    constraints: Some(vec![Percentage(30), Fill(1)]),
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
                LayoutNode {
                    widget: Some(UiWidget::Help),
                    direction: None,
                    constraints: None,
                    children: None,
                },
            ]),
        }
    }
}

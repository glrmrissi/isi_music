#[cfg(all(feature = "palette", feature = "album-art"))]
mod tests {
    use crate::app::App;
    use std::time::{Duration, Instant};

    use crate::app::theme_mgr::{ThemeChange, ThemeManager};
    use crate::ui::Ui;
    use crate::utils::debug_overlay::DebugOverlay;
    use crate::utils::palette::Rgb;
    use crate::utils::theme::{Theme, ThemeWatcher};

    fn make_reactive_theme_mgr() -> ThemeManager {
        let mut theme = Theme::default();
        theme.reactive_theme = true;
        theme.reactive_cross_fade_ms = 100;
        let watcher = ThemeWatcher::noop();
        ThemeManager::new(theme, watcher)
    }

    #[test]
    fn reactive_theme_start_sets_transition_state() {
        let mgr = make_reactive_theme_mgr();
        let debug = std::sync::Arc::new(DebugOverlay::new());
        let ui = Ui::new(mgr.theme.clone(), debug);

        let mut mgr = mgr;
        let swatches = vec![
            Rgb {
                r: 255,
                g: 100,
                b: 50,
            },
            Rgb {
                r: 50,
                g: 200,
                b: 100,
            },
        ];

        mgr.start_reactive(&swatches, &ui);

        assert!(mgr.reactive_start.is_some(), "reactive_start should be set");
        assert!(mgr.reactive_from.is_some(), "reactive_from should be set");
        assert!(
            mgr.reactive_target.is_some(),
            "reactive_target should be set"
        );
    }

    #[test]
    fn reactive_theme_lerp_produces_blended_theme() {
        let mut mgr = make_reactive_theme_mgr();
        let debug = std::sync::Arc::new(DebugOverlay::new());
        let ui = Ui::new(mgr.theme.clone(), debug.clone());

        let swatches = vec![
            Rgb {
                r: 255,
                g: 100,
                b: 50,
            },
            Rgb {
                r: 50,
                g: 200,
                b: 100,
            },
        ];

        mgr.start_reactive(&swatches, &ui);

        let base_bg = mgr.theme.background;

        let now = Instant::now() + Duration::from_millis(50);
        let blended = mgr.lerp_reactive(now);
        assert!(
            blended.is_some(),
            "lerp should return Some during transition"
        );

        let blended_theme = blended.unwrap();
        assert_ne!(
            blended_theme.background, base_bg,
            "blended bg should differ from base at t=0.5"
        );
        assert_ne!(
            blended_theme.background,
            mgr.reactive_target.as_ref().unwrap().background,
            "blended bg should not be at target yet at t=0.5"
        );
        let _ = blended_theme;
    }

    #[test]
    fn reactive_theme_lerp_completes_and_sets_target() {
        let mut mgr = make_reactive_theme_mgr();
        let debug = std::sync::Arc::new(DebugOverlay::new());
        let ui = Ui::new(mgr.theme.clone(), debug);

        let swatches = vec![
            Rgb {
                r: 255,
                g: 100,
                b: 50,
            },
            Rgb {
                r: 50,
                g: 200,
                b: 100,
            },
        ];

        mgr.start_reactive(&swatches, &ui);

        let target_bg = mgr.reactive_target.as_ref().unwrap().background;

        let now = Instant::now() + Duration::from_millis(200);
        let blended = mgr.lerp_reactive(now);
        assert!(blended.is_some(), "lerp should return Some at completion");

        let blended_theme = blended.unwrap();
        assert_eq!(
            blended_theme.background, target_bg,
            "final blended theme should match target"
        );
        assert!(mgr.reactive_start.is_none(), "start should be cleared");
        assert!(mgr.reactive_from.is_none(), "from should be cleared");
        assert!(mgr.reactive_target.is_none(), "target should be cleared");
        assert_eq!(
            mgr.theme.background, target_bg,
            "theme_mgr.theme should be the target after completion"
        );
    }

    #[test]
    fn reactive_theme_empty_swatches_does_nothing() {
        let mut mgr = make_reactive_theme_mgr();
        let debug = std::sync::Arc::new(DebugOverlay::new());
        let ui = Ui::new(mgr.theme.clone(), debug);

        let swatches: Vec<Rgb> = vec![];
        mgr.start_reactive(&swatches, &ui);

        assert!(
            mgr.reactive_start.is_none(),
            "empty swatches should not start"
        );
    }

    #[test]
    fn reactive_theme_store_and_clone_swatches() {
        let mut mgr = make_reactive_theme_mgr();
        let swatches = vec![Rgb {
            r: 10,
            g: 20,
            b: 30,
        }];
        mgr.store_swatches(swatches.clone());
        let cloned = mgr.swatches_clone();
        assert!(cloned.is_some(), "swatches should be stored");
        assert_eq!(cloned.unwrap().len(), 1);
    }

    #[test]
    fn reactive_theme_poll_returns_none_with_no_watcher_event() {
        let mut mgr = make_reactive_theme_mgr();
        let change = mgr.poll_theme_changes();
        match change {
            ThemeChange::None => {}
            ThemeChange::Apply { .. } => {
                panic!("poll should return None when no watcher event");
            }
        }
    }

    #[test]
    fn reactive_theme_toggle_pending_preserves_transition() {
        let (watcher, tx) = ThemeWatcher::with_sender();
        let mut theme = Theme::default();
        theme.reactive_theme = true;
        theme.reactive_cross_fade_ms = 100;
        let mut mgr = ThemeManager::new(theme, watcher);

        let debug = std::sync::Arc::new(DebugOverlay::new());
        let ui = Ui::new(mgr.theme.clone(), debug);

        let swatches = vec![Rgb {
            r: 255,
            g: 100,
            b: 50,
        }];
        mgr.start_reactive(&swatches, &ui);
        mgr.set_reactive_toggle_pending(true);

        let mut new_theme = mgr.theme.clone();
        new_theme.background = ratatui::style::Color::Rgb(10, 20, 30);
        tx.send(new_theme).unwrap();

        let change = mgr.poll_theme_changes();
        match change {
            ThemeChange::None => {}
            ThemeChange::Apply { .. } => {
                panic!(
                    "poll should not apply when toggle_pending is true and reactive_start is set"
                );
            }
        }
        assert!(
            !mgr.reactive_toggle_pending,
            "toggle_pending should be consumed"
        );
        assert!(
            mgr.reactive_start.is_some(),
            "transition should be preserved"
        );
    }

    #[test]
    fn reactive_theme_disable_clears_state() {
        let mut mgr = make_reactive_theme_mgr();
        let debug = std::sync::Arc::new(DebugOverlay::new());
        let ui = Ui::new(mgr.theme.clone(), debug);

        let swatches = vec![Rgb {
            r: 255,
            g: 100,
            b: 50,
        }];
        mgr.start_reactive(&swatches, &ui);

        let _restored = mgr.disable_reactive();
        assert!(mgr.reactive_start.is_none(), "disable should clear start");
        assert!(mgr.reactive_from.is_none(), "disable should clear from");
        assert!(mgr.reactive_target.is_none(), "disable should clear target");
        assert!(
            !mgr.reactive_toggle_pending,
            "disable should clear toggle_pending"
        );
    }

    #[test]
    fn reactive_theme_full_cycle_two_tracks() {
        let mut mgr = make_reactive_theme_mgr();
        let debug = std::sync::Arc::new(DebugOverlay::new());
        let mut ui = Ui::new(mgr.theme.clone(), debug.clone());

        let original_bg = mgr.theme.background;

        let swatches1 = vec![
            Rgb {
                r: 200,
                g: 50,
                b: 50,
            },
            Rgb {
                r: 50,
                g: 50,
                b: 200,
            },
        ];
        mgr.store_swatches(swatches1.clone());
        assert!(
            mgr.reactive_theme_enabled(),
            "reactive_theme should be enabled"
        );
        mgr.start_reactive(&swatches1, &ui);
        assert!(
            mgr.reactive_start.is_some(),
            "first transition should start"
        );

        let mut last_bg = original_bg;
        for i in 1..=10 {
            let now = Instant::now() + Duration::from_millis(i * 20);
            if let Some(blended) = mgr.lerp_reactive(now) {
                last_bg = blended.background;
                ui = Ui::new(blended, debug.clone());
            }
        }

        assert_ne!(
            last_bg, original_bg,
            "theme should change after first transition"
        );
        assert!(
            mgr.reactive_start.is_none(),
            "first transition should complete"
        );

        let first_reactive_bg = mgr.theme.background;
        let swatches2 = vec![
            Rgb {
                r: 50,
                g: 200,
                b: 50,
            },
            Rgb {
                r: 200,
                g: 200,
                b: 50,
            },
        ];
        mgr.store_swatches(swatches2.clone());
        mgr.start_reactive(&swatches2, &ui);
        assert!(
            mgr.reactive_start.is_some(),
            "second transition should start"
        );

        for i in 1..=10 {
            let now = Instant::now() + Duration::from_millis(200 + i * 20);
            if let Some(blended) = mgr.lerp_reactive(now) {
                last_bg = blended.background;
                ui = Ui::new(blended, debug.clone());
            }
        }

        assert_ne!(
            last_bg, first_reactive_bg,
            "second track should produce different theme"
        );
        assert!(
            mgr.reactive_start.is_none(),
            "second transition should complete"
        );
    }

    #[tokio::test]
    async fn reactive_theme_app_integration_start_reactive_via_swatches() {
        let mut app = App::new_for_test().await;

        app.theme_mgr.theme.reactive_theme = true;
        app.theme_mgr.theme.reactive_cross_fade_ms = 100;

        let debug = std::sync::Arc::new(DebugOverlay::new());
        let ui = Ui::new(app.theme_mgr.theme.clone(), debug);

        let swatches = vec![
            Rgb {
                r: 200,
                g: 50,
                b: 50,
            },
            Rgb {
                r: 50,
                g: 50,
                b: 200,
            },
        ];
        app.theme_mgr.store_swatches(swatches.clone());
        app.theme_mgr.start_reactive(&swatches, &ui);

        assert!(
            app.theme_mgr.reactive_start.is_some(),
            "app.theme_mgr should have reactive transition started"
        );

        let now = Instant::now() + Duration::from_millis(200);
        let blended = app.theme_mgr.lerp_reactive(now);
        assert!(blended.is_some(), "lerp should complete");
        assert!(
            app.theme_mgr.reactive_start.is_none(),
            "transition should complete after lerp"
        );
    }
}

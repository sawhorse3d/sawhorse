//! Whether the window fits the screens at the size it is set to open at (issue #79).
//!
//! `tauri.conf.json` opens the window 1440 by 900. On a 1280 by 800 screen KWin (KDE Plasma)
//! leaves it at that size, so the right end of the title bar, with minimise, maximise and
//! close, is off the screen. Mutter (GNOME) and Muffin (Cinnamon) make such a window fit by
//! themselves.
//!
//! Tauri's own window setting for this, `preventOverflow`, was tried first and does nothing
//! under Wayland: it sizes the window against the monitor GDK calls primary, and under
//! Wayland GDK names none. So the shell asks for every monitor instead and hands their work
//! areas to [`fit`] before the window is first drawn (`src-tauri/src/lib.rs`, `fit_window`).
//!
//! The rule:
//!
//! - The window fits when it is no wider than the narrowest screen, and the shortest screen
//!   is taller than it by [`ROOM`] or more. Then nothing is done: [`Fit::AsConfigured`].
//!   The room is for the title bar and a panel. Under Wayland GDK gives a monitor's whole
//!   area as its work area, so a 1440 by 900 window "fits" a 1440 by 900 screen by the
//!   numbers, and on KDE Plasma it opened with its bottom 74 px below the panel's top edge.
//! - Otherwise it opens maximised: [`Fit::Maximized`]. The desktop then gives it the size
//!   that fits, around its own panels and with its own title bar, which the shell cannot
//!   work out for itself: no desktop says how tall its title bar will be before the window
//!   is up, and under Wayland none says where its panels are.
//! - Maximised, it is also given a smaller size to go back to when the user un-maximises it:
//!   [`RESTORE_NUM`]/[`RESTORE_DEN`] of the screen each way, no larger than the size it was
//!   set to open at and no smaller than its minimum.
//!
//! Every screen counts, not only the one the window will open on, because the shell does not
//! know which that is: under Wayland a window is not told where it is, and on any system the
//! desktop places it. With one screen there is nothing to choose. With several, a window
//! that would not fit the smallest opens maximised on whichever it lands on.
//!
//! With no screen reported, or only screens of no size, nothing is done.
//!
//! All sizes here are in logical pixels, the unit `tauri.conf.json` gives the window's size
//! in. [`logical`] turns a monitor's physical pixels into them.

/// A width and a height, in logical pixels.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Size {
    pub width: u32,
    pub height: u32,
}

impl Size {
    pub const fn new(width: u32, height: u32) -> Self {
        Self { width, height }
    }
}

/// What to do with the window before it is first drawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Fit {
    /// It fits every screen: leave it at the size it is set to open at.
    AsConfigured,
    /// It does not fit: open it maximised, with `restore` as the size it goes back to when
    /// it is un-maximised.
    Maximized { restore: Size },
}

/// How much taller than the window a screen has to be for the window to be left as it is,
/// in logical pixels: room for a title bar and a panel, neither of which the shell is told
/// about under Wayland. The desktops measured took 58 to 74 px for the two (KWin: a panel of
/// 30 or 46 px and a title bar of 28; Cinnamon: 40 and 32).
pub const ROOM: u32 = 100;

/// The size to go back to from maximised is this part of the screen, each way: seven
/// eighths. Of an 800 px screen that leaves 100 px, as [`ROOM`] does.
pub const RESTORE_NUM: u32 = 7;
pub const RESTORE_DEN: u32 = 8;

/// A monitor's `physical` pixel count in logical pixels, at its scale factor `scale`.
/// Rounded down, so a screen is never taken for larger than it is. A scale factor that is
/// not a number above zero is taken as 1.
pub fn logical(physical: u32, scale: f64) -> u32 {
    if !scale.is_finite() || scale <= 0.0 {
        return physical;
    }
    // `as` saturates, so a tiny scale cannot wrap.
    (f64::from(physical) / scale).floor() as u32
}

/// The narrowest width and the shortest height among `screens`, leaving out screens with no
/// width or no height. `None` when none is left.
fn smallest(screens: &[Size]) -> Option<Size> {
    screens
        .iter()
        .filter(|s| s.width > 0 && s.height > 0)
        .fold(None, |least: Option<Size>, s| {
            Some(match least {
                None => *s,
                Some(l) => Size::new(l.width.min(s.width), l.height.min(s.height)),
            })
        })
}

/// One side of the size to go back to: [`RESTORE_NUM`]/[`RESTORE_DEN`] of the screen's side,
/// no more than the side the window was set to open at and no less than its minimum.
fn restore_side(wanted: u32, least: u32, screen: u32) -> u32 {
    let part = (u64::from(screen) * u64::from(RESTORE_NUM) / u64::from(RESTORE_DEN)) as u32;
    part.min(wanted).max(least)
}

/// What to do with a window set to open at `wanted`, whose smallest allowed size is `least`,
/// on a system whose monitors have the work areas `screens`.
pub fn fit(wanted: Size, least: Size, screens: &[Size]) -> Fit {
    let Some(screen) = smallest(screens) else {
        return Fit::AsConfigured;
    };
    if wanted.width <= screen.width && wanted.height.saturating_add(ROOM) <= screen.height {
        return Fit::AsConfigured;
    }
    Fit::Maximized {
        restore: Size::new(
            restore_side(wanted.width, least.width, screen.width),
            restore_side(wanted.height, least.height, screen.height),
        ),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The window as `tauri.conf.json` sets it: 1440 by 900, at least 900 by 600.
    const WANTED: Size = Size::new(1440, 900);
    const LEAST: Size = Size::new(900, 600);

    fn on(screens: &[Size]) -> Fit {
        fit(WANTED, LEAST, screens)
    }

    #[test]
    fn a_screen_larger_than_the_window_leaves_it_as_configured() {
        assert_eq!(on(&[Size::new(1920, 1080)]), Fit::AsConfigured);
        assert_eq!(on(&[Size::new(2560, 1440)]), Fit::AsConfigured);
        assert_eq!(on(&[Size::new(3840, 2160)]), Fit::AsConfigured);
    }

    #[test]
    fn a_screen_as_wide_as_the_window_and_taller_by_the_room_leaves_it_as_configured() {
        assert_eq!(on(&[Size::new(1440, 1000)]), Fit::AsConfigured);
    }

    #[test]
    fn a_screen_exactly_the_windows_size_opens_maximised() {
        // KDE Plasma under Wayland, measured: the window opened 1440 by 928 with its title
        // bar, on a work area of 1440 by 854.
        assert_eq!(
            on(&[WANTED]),
            Fit::Maximized {
                restore: Size::new(1260, 787)
            }
        );
    }

    #[test]
    fn the_larger_screens_measured_leave_it_as_configured() {
        // 1920 by 1080: the whole screen as GDK gives it under Wayland, and the work areas
        // KWin and Cinnamon's window manager gave.
        for screen in [
            Size::new(1920, 1080),
            Size::new(1920, 1034),
            Size::new(1920, 1050),
            Size::new(1920, 1040),
        ] {
            assert_eq!(on(&[screen]), Fit::AsConfigured, "{screen:?}");
        }
    }

    #[test]
    fn the_issues_screen_1280_by_800_opens_maximised_with_1120_by_700_to_go_back_to() {
        assert_eq!(
            on(&[Size::new(1280, 800)]),
            Fit::Maximized {
                restore: Size::new(1120, 700)
            }
        );
    }

    #[test]
    fn the_work_areas_measured_on_the_desktops_tried_open_maximised() {
        // On a 1280 by 800 screen: KWin's own work area on the two Plasma systems,
        // Cinnamon's under X11, and the whole screen that GDK reports under Wayland. Then
        // Cinnamon's on a 1440 by 900 screen.
        for screen in [
            Size::new(1280, 754),
            Size::new(1280, 770),
            Size::new(1280, 760),
            Size::new(1280, 800),
            Size::new(1440, 860),
        ] {
            assert!(matches!(on(&[screen]), Fit::Maximized { .. }), "{screen:?}");
        }
    }

    #[test]
    fn one_pixel_short_either_way_opens_maximised() {
        assert!(matches!(
            on(&[Size::new(1439, 1000)]),
            Fit::Maximized { .. }
        ));
        assert!(matches!(on(&[Size::new(1440, 999)]), Fit::Maximized { .. }));
        // Wide enough and not tall enough, and the other way round.
        assert!(matches!(on(&[Size::new(3840, 800)]), Fit::Maximized { .. }));
        assert!(matches!(
            on(&[Size::new(1280, 2160)]),
            Fit::Maximized { .. }
        ));
    }

    #[test]
    fn no_screen_or_only_screens_of_no_size_leaves_it_as_configured() {
        assert_eq!(on(&[]), Fit::AsConfigured);
        assert_eq!(on(&[Size::new(0, 0)]), Fit::AsConfigured);
        assert_eq!(
            on(&[Size::new(0, 1080), Size::new(1920, 0)]),
            Fit::AsConfigured
        );
    }

    #[test]
    fn a_screen_of_no_size_beside_a_real_one_is_left_out() {
        assert_eq!(
            on(&[Size::new(0, 0), Size::new(1920, 1080)]),
            Fit::AsConfigured
        );
        assert_eq!(
            on(&[Size::new(1280, 800), Size::new(0, 0)]),
            Fit::Maximized {
                restore: Size::new(1120, 700)
            }
        );
    }

    #[test]
    fn with_several_screens_the_narrowest_and_the_shortest_decide() {
        // A large screen beside a laptop's: the window may land on either.
        assert_eq!(
            on(&[Size::new(2560, 1440), Size::new(1280, 800)]),
            Fit::Maximized {
                restore: Size::new(1120, 700)
            }
        );
        // In either order.
        assert_eq!(
            on(&[Size::new(1280, 800), Size::new(2560, 1440)]),
            on(&[Size::new(2560, 1440), Size::new(1280, 800)])
        );
        // The narrowest and the shortest need not be the same screen: one on its side.
        assert_eq!(
            on(&[Size::new(1080, 1920), Size::new(1920, 1080)]),
            Fit::Maximized {
                restore: Size::new(945, 900)
            }
        );
        // Two that it fits.
        assert_eq!(
            on(&[Size::new(1920, 1080), Size::new(1440, 1000)]),
            Fit::AsConfigured
        );
    }

    #[test]
    fn the_size_to_go_back_to_is_never_below_the_windows_minimum() {
        // 1024 by 600: seven eighths is 896 by 525, under the minimum both ways.
        assert_eq!(
            on(&[Size::new(1024, 600)]),
            Fit::Maximized {
                restore: Size::new(900, 600)
            }
        );
    }

    #[test]
    fn the_size_to_go_back_to_is_never_above_the_size_the_window_opens_at() {
        // Wide enough for seven eighths to pass 1440; the height is what does not fit.
        assert_eq!(
            on(&[Size::new(3840, 800)]),
            Fit::Maximized {
                restore: Size::new(1440, 700)
            }
        );
    }

    #[test]
    fn a_very_large_screen_or_window_does_not_overflow_the_arithmetic() {
        assert_eq!(on(&[Size::new(u32::MAX, u32::MAX)]), Fit::AsConfigured);
        // A window as tall as a number can be: the room added to it does not wrap to fit.
        assert_eq!(
            fit(Size::new(1440, u32::MAX), LEAST, &[Size::new(1920, 1080)]),
            Fit::Maximized {
                restore: Size::new(1440, 945)
            }
        );
        assert_eq!(
            on(&[Size::new(u32::MAX, 1)]),
            Fit::Maximized {
                restore: Size::new(1440, 600)
            }
        );
    }

    /// Over every screen from 1 by 1 to 3000 by 2000 in steps of 7 and 5 (and the sizes one
    /// either side of the window's own), what [`fit`] says holds to the rule.
    #[test]
    fn over_a_sweep_of_screens_the_answer_holds_to_the_rule() {
        let mut widths: Vec<u32> = (1..=3000).step_by(7).collect();
        widths.extend([899, 900, 901, 1439, 1440, 1441]);
        let mut heights: Vec<u32> = (1..=2000).step_by(5).collect();
        heights.extend([599, 600, 601, 899, 900, 901, 999, 1000, 1001]);
        let (mut kept, mut maximised) = (0u32, 0u32);
        for &w in &widths {
            for &h in &heights {
                let screen = Size::new(w, h);
                match on(&[screen]) {
                    Fit::AsConfigured => {
                        kept += 1;
                        assert!(w >= WANTED.width && h >= WANTED.height + ROOM, "{screen:?}");
                    }
                    Fit::Maximized { restore } => {
                        maximised += 1;
                        assert!(w < WANTED.width || h < WANTED.height + ROOM, "{screen:?}");
                        assert!(restore.width <= WANTED.width, "{screen:?}");
                        assert!(restore.height <= WANTED.height, "{screen:?}");
                        assert!(restore.width >= LEAST.width, "{screen:?}");
                        assert!(restore.height >= LEAST.height, "{screen:?}");
                        // Inside the screen wherever the minimum allows it.
                        assert!(restore.width <= w.max(LEAST.width), "{screen:?}");
                        assert!(restore.height <= h.max(LEAST.height), "{screen:?}");
                    }
                }
            }
        }
        assert!(kept > 0 && maximised > 0, "{kept} {maximised}");
    }

    #[test]
    fn a_window_with_no_minimum_goes_back_to_seven_eighths_of_a_small_screen() {
        assert_eq!(
            fit(WANTED, Size::new(0, 0), &[Size::new(800, 480)]),
            Fit::Maximized {
                restore: Size::new(700, 420)
            }
        );
    }

    #[test]
    fn logical_pixels_from_physical_ones() {
        assert_eq!(logical(1280, 1.0), 1280);
        assert_eq!(logical(2560, 2.0), 1280);
        assert_eq!(logical(1920, 1.25), 1536);
        assert_eq!(logical(1920, 1.5), 1280);
        // Rounded down: 1366 / 1.25 is 1092.8.
        assert_eq!(logical(1366, 1.25), 1092);
        assert_eq!(logical(0, 2.0), 0);
    }

    #[test]
    fn a_scale_factor_that_is_not_a_number_above_zero_is_taken_as_one() {
        for scale in [0.0, -1.0, f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            assert_eq!(logical(1280, scale), 1280, "{scale}");
        }
    }

    #[test]
    fn a_tiny_scale_factor_does_not_wrap() {
        assert_eq!(logical(u32::MAX, f64::MIN_POSITIVE), u32::MAX);
    }
}

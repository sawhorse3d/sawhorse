//! Holding a Linux desktop's logout while the model has unsaved changes (#73).
//!
//! KDE Plasma closes each window when the session ends, so the question the window asks
//! before it closes (`sawhorse-close-guard`) shows there at logout, restart and shutdown.
//! GNOME and Cinnamon end the session without closing the windows first, so on them nothing
//! was asked. Both expect an app with unsaved work to hold a logout inhibitor with the
//! session manager instead (`org.gnome.SessionManager`, the name cinnamon-session owns too).
//! While one is held, the desktop's own logout dialog names the app and the reason.
//!
//! [`Hold`] is the shell's memory of that inhibitor: a pure state machine the shell
//! (`src-tauri/src/logout_hold.rs`) feeds with what the page says about unsaved changes and
//! with the outcome of each call it makes on the session bus. Its rules:
//!
//! - Unsaved, nothing held: take an inhibitor ([`Step::Take`]).
//! - Unsaved again while one is held: nothing. One inhibitor, however many edits.
//! - Saved (or the window is going) while one is held: release it ([`Step::Release`]).
//! - A desktop with no such session manager ([`Took::Absent`]: KDE Plasma and others) is
//!   not asked again while Sawhorse runs.
//! - A call that failed for another reason ([`Took::Failed`]) holds nothing and is tried
//!   again the next time the model becomes unsaved.
//!
//! The session manager drops the inhibitor by itself when Sawhorse leaves the session bus,
//! so one that is never released (a crash, a window destroyed) does not outlive the app.

/// The session manager's name on the session bus (gnome-session and cinnamon-session).
pub const BUS_NAME: &str = "org.gnome.SessionManager";
/// The session manager's object.
pub const OBJECT_PATH: &str = "/org/gnome/SessionManager";
/// The interface `Inhibit` and `Uninhibit` belong to.
pub const INTERFACE: &str = "org.gnome.SessionManager";
/// The app's name to the session manager: `productName` in `tauri.conf.json`. GNOME's logout
/// dialog lists an inhibitor only when it finds an installed desktop file by this name; with
/// the .deb and .rpm it did on GNOME 46 to 51 (#119, tried by hand).
pub const APP_ID: &str = "Sawhorse";
/// What the logout dialog shows under the app's name.
pub const REASON: &str = "This model has unsaved changes";
/// The session manager's flag for "inhibit logging out" (it covers restart and shutdown).
pub const LOGOUT: u32 = 1;

/// What each word the page sends the `close_guard` command (`sawhorse_close_guard::Say`)
/// means for the hold: `Some(true)` the model has unsaved changes, `Some(false)` it has none
/// any more or the window is going, `None` nothing for the hold.
pub fn word(word: &str) -> Option<bool> {
    match word {
        "unsaved" => Some(true),
        // "close": the person said Yes to discarding (or nothing was unsaved by then), and
        // the window is destroyed next.
        "saved" | "close" => Some(false),
        _ => None,
    }
}

/// What the shell does after the page has spoken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Nothing.
    Nothing,
    /// Ask the session manager for an inhibitor, then report the outcome to [`Hold::took`].
    Take,
    /// Give the inhibitor with this cookie back.
    Release(u32),
}

/// How a [`Step::Take`] ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Took {
    /// The session manager gave an inhibitor with this cookie.
    Held(u32),
    /// There is no session bus, or no session manager of this kind on it.
    Absent,
    /// The call failed for another reason (the session manager refused it, the answer made
    /// no sense, the bus did not say in time whether a session manager is on it).
    Failed,
}

/// The shell's memory of the logout inhibitor.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Hold {
    /// The inhibitor held now.
    cookie: Option<u32>,
    /// This desktop has no session manager to ask: never ask again.
    absent: bool,
}

impl Hold {
    /// Nothing held and nothing known about the desktop.
    pub fn new() -> Hold {
        Hold::default()
    }

    /// The page says the model has unsaved changes.
    pub fn unsaved(&mut self) -> Step {
        if self.absent || self.cookie.is_some() {
            Step::Nothing
        } else {
            Step::Take
        }
    }

    /// The outcome of the [`Step::Take`] that [`Hold::unsaved`] asked for.
    pub fn took(&mut self, took: Took) {
        match took {
            Took::Held(cookie) => self.cookie = Some(cookie),
            Took::Absent => self.absent = true,
            Took::Failed => {}
        }
    }

    /// The page says nothing is unsaved any more (saved, opened, new), or the window is going.
    pub fn saved(&mut self) -> Step {
        match self.cookie.take() {
            Some(cookie) => Step::Release(cookie),
            None => Step::Nothing,
        }
    }

    /// Whether an inhibitor is held now.
    pub fn held(&self) -> bool {
        self.cookie.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_first_unsaved_takes_and_the_save_releases_the_same_cookie() {
        let mut h = Hold::new();
        assert_eq!(h.unsaved(), Step::Take);
        h.took(Took::Held(7));
        assert!(h.held());
        assert_eq!(h.saved(), Step::Release(7));
        assert!(!h.held());
    }

    #[test]
    fn unsaved_again_while_held_takes_no_second_inhibitor() {
        let mut h = Hold::new();
        assert_eq!(h.unsaved(), Step::Take);
        h.took(Took::Held(7));
        assert_eq!(h.unsaved(), Step::Nothing);
        assert_eq!(h.unsaved(), Step::Nothing);
        assert_eq!(h.saved(), Step::Release(7));
    }

    #[test]
    fn saved_with_nothing_held_releases_nothing() {
        let mut h = Hold::new();
        assert_eq!(h.saved(), Step::Nothing);
        assert_eq!(h.unsaved(), Step::Take);
        h.took(Took::Held(3));
        assert_eq!(h.saved(), Step::Release(3));
        assert_eq!(h.saved(), Step::Nothing);
    }

    #[test]
    fn each_unsaved_spell_takes_its_own_inhibitor() {
        let mut h = Hold::new();
        assert_eq!(h.unsaved(), Step::Take);
        h.took(Took::Held(1));
        assert_eq!(h.saved(), Step::Release(1));
        assert_eq!(h.unsaved(), Step::Take);
        h.took(Took::Held(2));
        assert_eq!(h.saved(), Step::Release(2));
    }

    #[test]
    fn a_desktop_with_no_session_manager_is_asked_once() {
        let mut h = Hold::new();
        assert_eq!(h.unsaved(), Step::Take);
        h.took(Took::Absent);
        assert!(!h.held());
        assert_eq!(h.saved(), Step::Nothing);
        assert_eq!(h.unsaved(), Step::Nothing);
        assert_eq!(h.saved(), Step::Nothing);
    }

    #[test]
    fn a_failed_call_holds_nothing_and_is_tried_again() {
        let mut h = Hold::new();
        assert_eq!(h.unsaved(), Step::Take);
        h.took(Took::Failed);
        assert!(!h.held());
        assert_eq!(h.saved(), Step::Nothing);
        assert_eq!(h.unsaved(), Step::Take);
        h.took(Took::Held(9));
        assert_eq!(h.saved(), Step::Release(9));
    }

    /// Every sequence of the page's words six steps deep, with every outcome of each take:
    /// a release always names the cookie of the one inhibitor held, a take is asked for only
    /// while none is held, and never again once the desktop has none to give.
    #[test]
    fn never_two_inhibitors_and_never_a_release_of_one_not_held() {
        fn walk(h: Hold, held: Option<u32>, absent: bool, next: u32, depth: u32) {
            if depth == 0 {
                return;
            }
            // Saved.
            let mut s = h.clone();
            match s.saved() {
                Step::Release(c) => assert_eq!(Some(c), held),
                Step::Nothing => assert_eq!(held, None),
                Step::Take => panic!("saved never takes"),
            }
            walk(s, None, absent, next, depth - 1);
            // Unsaved, with each outcome.
            let mut u = h.clone();
            match u.unsaved() {
                Step::Nothing => {
                    assert!(absent || held.is_some());
                    walk(u, held, absent, next, depth - 1);
                }
                Step::Take => {
                    assert!(!absent && held.is_none());
                    for took in [Took::Held(next), Took::Absent, Took::Failed] {
                        let mut t = u.clone();
                        t.took(took);
                        let now = if let Took::Held(c) = took {
                            Some(c)
                        } else {
                            None
                        };
                        assert_eq!(t.held(), now.is_some());
                        walk(t, now, took == Took::Absent, next + 1, depth - 1);
                    }
                }
                Step::Release(_) => panic!("unsaved never releases"),
            }
        }
        walk(Hold::new(), None, false, 1, 6);
    }

    #[test]
    fn unsaved_takes_and_saved_or_the_window_going_gives_back() {
        assert_eq!(word("unsaved"), Some(true));
        assert_eq!(word("saved"), Some(false));
        assert_eq!(word("close"), Some(false));
        for none in ["keep", "failed", "exit", "", "Unsaved"] {
            assert_eq!(word(none), None, "{none:?}");
        }
    }

    /// The session manager's inhibit flags are 1 logging out, 2 switching user, 4 suspending,
    /// 8 the session being marked idle, 16 automounting: only logging out is held.
    #[test]
    fn only_logging_out_is_held() {
        assert_eq!(LOGOUT, 1);
    }

    /// The words `sawhorse_close_guard::Say::parse` accepts, read from its source: a word
    /// added there must be given a rule here (one that means the window is going must give
    /// the hold back).
    #[test]
    fn every_word_close_guard_hears_has_a_rule_here() {
        let src = include_str!("../../close-guard/src/lib.rs");
        let heard: Vec<&str> = src
            .lines()
            .filter_map(|line| {
                let (w, rest) = line.trim().strip_prefix('"')?.split_once('"')?;
                rest.trim_start().starts_with("=> Say::").then_some(w)
            })
            .collect();
        assert_eq!(
            heard,
            ["unsaved", "saved", "keep", "close", "failed", "exit"],
            "close-guard's words changed: give each new one its rule in logout-hold's word()"
        );
    }

    /// GNOME lists the inhibitor only when it finds the app's installed desktop file by this
    /// name, and the bundler makes the packages' desktop file from `productName`.
    #[test]
    fn the_app_id_is_the_product_name_the_desktop_file_is_named_after() {
        let conf = include_str!("../../tauri.conf.json");
        assert!(
            conf.contains(&format!("\"productName\": \"{APP_ID}\"")),
            "tauri.conf.json's productName is no longer {APP_ID:?}: the desktop file has another name"
        );
    }
}

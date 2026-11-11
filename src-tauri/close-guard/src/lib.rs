//! The question before the desktop window closes over unsaved changes (lane 35, #44).
//!
//! Before lane 35 the window's X, Alt+F4 and every other close request closed Sawhorse at
//! once, unsaved changes and all. Now the shell asks the page first when the page has said
//! there are unsaved changes, and the page asks the person the question File > New asks.
//!
//! The page cannot be trusted to answer: its script can be frozen, crashed or broken, and the
//! question's dialog can fail to open. Tauri's own close-requested listener for the page
//! (`onCloseRequested`) would have the shell keep the window open on every request and wait
//! for the page to destroy it, so a page that never answers would hold the window open until
//! the Task Manager ended it. [`Guard`] is the shell's side instead, a pure state machine the
//! shell (`src-tauri/src/lib.rs`) feeds with every close request and everything the page
//! says, with these rules:
//!
//! - Nothing unsaved (or a page that never said anything): the window closes at once.
//! - Unsaved, no question out: the window stays and the page is asked ([`Request::Ask`]).
//! - Unsaved, a question out for less than [`PATIENCE_MS`]: the window stays and nothing
//!   more is sent ([`Request::Wait`]); a double click on the X asks once.
//! - Unsaved, a question out for [`PATIENCE_MS`] or longer: the window closes without
//!   waiting for the answer. A page that is frozen, a dialog that never opened and an answer
//!   that never arrived all end here, so a second close request two seconds after the first
//!   always closes the window.
//! - After the page said it could not ask ([`Say::Failed`]): the next request closes at once.
//!
//! The page's answers: [`Say::Keep`] (No: the window and the model stay), [`Say::Close`]
//! (Yes: the shell destroys the window), and [`Say::Exit`] (File > Exit), which the shell
//! turns into an ordinary close request, so Exit takes the X's path exactly.

/// How long a question may go unanswered before another close request stops waiting for it.
pub const PATIENCE_MS: u64 = 2_000;

/// What the shell does with one close request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Request {
    /// Let the window close.
    Close,
    /// Keep the window open and ask the page (the `sawhorse://close-requested` event).
    Ask,
    /// Keep the window open: the page was asked a moment ago.
    Wait,
}

/// What the page says, through the shell's `close_guard` command.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Say {
    /// The model has changes that are not saved.
    Unsaved,
    /// The model has no unsaved changes (saved, opened, new, or the page just started).
    Saved,
    /// The person answered No: keep the window and the model.
    Keep,
    /// The person answered Yes (or nothing was unsaved by the time the page looked): close.
    Close,
    /// The page could not ask (the dialog failed, the handler threw): the next request closes.
    Failed,
    /// File > Exit: close the window the way its X does.
    Exit,
}

impl Say {
    /// The page's word for each message, as the `close_guard` command receives it.
    pub fn parse(word: &str) -> Option<Say> {
        Some(match word {
            "unsaved" => Say::Unsaved,
            "saved" => Say::Saved,
            "keep" => Say::Keep,
            "close" => Say::Close,
            "failed" => Say::Failed,
            "exit" => Say::Exit,
            _ => return None,
        })
    }
}

/// What the shell does after the page has spoken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Then {
    /// Nothing.
    Nothing,
    /// Destroy the window (no further close request: the person said Yes).
    Destroy,
    /// Send the window an ordinary close request (`Window::close`), which comes back through
    /// [`Guard::request`] like the X.
    RequestClose,
}

/// The shell's memory of the close question.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Guard {
    /// The page's last word on unsaved changes.
    unsaved: bool,
    /// When the page was last asked and has not answered (milliseconds on the shell's clock).
    asked_at: Option<u64>,
    /// The page could not ask: the next request closes.
    gave_up: bool,
}

impl Guard {
    /// A window that has heard nothing from its page: it closes on request, as before lane 35.
    pub fn new() -> Guard {
        Guard::default()
    }

    /// A close request at `now_ms` (any monotonic clock in milliseconds).
    pub fn request(&mut self, now_ms: u64) -> Request {
        if !self.unsaved || self.gave_up {
            return Request::Close;
        }
        match self.asked_at {
            None => {
                self.asked_at = Some(now_ms);
                Request::Ask
            }
            Some(at) if now_ms.saturating_sub(at) < PATIENCE_MS => Request::Wait,
            Some(_) => Request::Close,
        }
    }

    /// The page says something.
    pub fn say(&mut self, say: Say) -> Then {
        match say {
            Say::Unsaved => self.unsaved = true,
            Say::Saved => {
                self.unsaved = false;
                self.asked_at = None;
                self.gave_up = false;
            }
            Say::Keep => self.asked_at = None,
            Say::Close => {
                self.asked_at = None;
                return Then::Destroy;
            }
            Say::Failed => {
                self.asked_at = None;
                self.gave_up = true;
            }
            Say::Exit => return Then::RequestClose,
        }
        Then::Nothing
    }

    /// Whether a question is out and unanswered.
    pub fn asking(&self) -> bool {
        self.asked_at.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unsaved() -> Guard {
        let mut g = Guard::new();
        assert_eq!(g.say(Say::Unsaved), Then::Nothing);
        g
    }

    #[test]
    fn a_window_whose_page_said_nothing_closes_at_once() {
        let mut g = Guard::new();
        assert_eq!(g.request(0), Request::Close);
        assert!(!g.asking());
    }

    #[test]
    fn a_clean_model_closes_without_a_question() {
        let mut g = unsaved();
        g.say(Say::Saved);
        assert_eq!(g.request(10), Request::Close);
        assert!(!g.asking());
    }

    #[test]
    fn unsaved_changes_ask_the_page_and_keep_the_window() {
        let mut g = unsaved();
        assert_eq!(g.request(100), Request::Ask);
        assert!(g.asking());
    }

    #[test]
    fn no_keeps_the_window_and_the_next_close_asks_again() {
        let mut g = unsaved();
        assert_eq!(g.request(0), Request::Ask);
        assert_eq!(g.say(Say::Keep), Then::Nothing);
        assert!(!g.asking());
        // Still unsaved: asked again, even long after (the answer was a person's).
        assert_eq!(g.request(60_000), Request::Ask);
    }

    #[test]
    fn yes_destroys_the_window() {
        let mut g = unsaved();
        assert_eq!(g.request(0), Request::Ask);
        assert_eq!(g.say(Say::Close), Then::Destroy);
    }

    #[test]
    fn a_second_request_while_the_question_is_out_waits_then_closes() {
        let mut g = unsaved();
        assert_eq!(g.request(1_000), Request::Ask);
        // A double click on the X, or a second Alt+F4: one question.
        assert_eq!(g.request(1_001), Request::Wait);
        assert_eq!(g.request(1_000 + PATIENCE_MS - 1), Request::Wait);
        // Two seconds without an answer: the page is not answering. Close.
        assert_eq!(g.request(1_000 + PATIENCE_MS), Request::Close);
    }

    #[test]
    fn waiting_does_not_restart_the_patience() {
        let mut g = unsaved();
        assert_eq!(g.request(0), Request::Ask);
        for t in (100..PATIENCE_MS).step_by(100) {
            assert_eq!(g.request(t), Request::Wait, "at {t} ms");
        }
        assert_eq!(g.request(PATIENCE_MS), Request::Close);
    }

    #[test]
    fn a_page_that_never_answers_never_holds_the_window() {
        // Frozen, crashed, or its listener never registered: the page reported unsaved
        // changes once and then said nothing.
        let mut g = unsaved();
        assert_eq!(g.request(5_000), Request::Ask);
        assert_eq!(g.request(5_000 + 10 * PATIENCE_MS), Request::Close);
    }

    #[test]
    fn a_page_that_could_not_ask_lets_the_next_request_close_at_once() {
        let mut g = unsaved();
        assert_eq!(g.request(0), Request::Ask);
        assert_eq!(g.say(Say::Failed), Then::Nothing);
        assert!(!g.asking());
        assert_eq!(g.request(1), Request::Close);
    }

    #[test]
    fn saving_after_a_failed_question_asks_again_next_time() {
        let mut g = unsaved();
        g.request(0);
        g.say(Say::Failed);
        g.say(Say::Saved);
        g.say(Say::Unsaved);
        assert_eq!(g.request(10), Request::Ask);
    }

    #[test]
    fn saving_clears_a_question_left_out() {
        // A question out, then the model saved and changed again much later: the old question
        // must not make the next request close without asking.
        let mut g = unsaved();
        assert_eq!(g.request(0), Request::Ask);
        g.say(Say::Saved);
        g.say(Say::Unsaved);
        assert_eq!(g.request(10 * PATIENCE_MS), Request::Ask);
    }

    #[test]
    fn exit_is_an_ordinary_close_request() {
        let mut g = unsaved();
        assert_eq!(g.say(Say::Exit), Then::RequestClose);
        // Saying Exit changes nothing by itself: the request it causes is decided as the X's.
        assert_eq!(g, unsaved());
        let mut clean = Guard::new();
        assert_eq!(clean.say(Say::Exit), Then::RequestClose);
        assert_eq!(clean.request(0), Request::Close);
    }

    #[test]
    fn a_clock_that_steps_back_waits_instead_of_closing() {
        let mut g = unsaved();
        assert_eq!(g.request(10_000), Request::Ask);
        assert_eq!(g.request(5), Request::Wait);
    }

    #[test]
    fn the_page_words_parse_and_nothing_else_does() {
        for (w, s) in [
            ("unsaved", Say::Unsaved),
            ("saved", Say::Saved),
            ("keep", Say::Keep),
            ("close", Say::Close),
            ("failed", Say::Failed),
            ("exit", Say::Exit),
        ] {
            assert_eq!(Say::parse(w), Some(s));
        }
        for w in ["", "Close", "yes", "no", "destroy", "close ", "asking"] {
            assert_eq!(Say::parse(w), None, "{w:?}");
        }
    }

    /// Everything the page can say short of closing the window, and close requests at a few
    /// spacings: every sequence of up to six of them, from a fresh guard.
    fn every_state(mut visit: impl FnMut(&Guard, u64, &[String])) {
        #[derive(Clone, Copy)]
        enum Step {
            Say(Say),
            Request(u64),
        }
        let steps = [
            Step::Say(Say::Unsaved),
            Step::Say(Say::Saved),
            Step::Say(Say::Keep),
            Step::Say(Say::Failed),
            Step::Request(0),
            Step::Request(1),
            Step::Request(PATIENCE_MS - 1),
            Step::Request(PATIENCE_MS),
        ];
        fn walk(
            g: &Guard,
            now: u64,
            path: &mut Vec<String>,
            depth: usize,
            steps: &[Step],
            visit: &mut dyn FnMut(&Guard, u64, &[String]),
        ) {
            visit(g, now, path);
            if depth == 0 {
                return;
            }
            for s in steps {
                let mut next = g.clone();
                let mut t = now;
                match *s {
                    Step::Say(say) => {
                        next.say(say);
                        path.push(format!("{say:?}"));
                    }
                    Step::Request(dt) => {
                        t += dt;
                        // A request that closes ends the window: no state follows it.
                        if next.request(t) == Request::Close {
                            continue;
                        }
                        path.push(format!("request@{t}"));
                    }
                }
                walk(&next, t, path, depth - 1, steps, visit);
                path.pop();
            }
        }
        walk(&Guard::new(), 0, &mut Vec::new(), 6, &steps, &mut visit);
    }

    #[test]
    fn never_trapped_two_requests_two_seconds_apart_always_close() {
        let mut states = 0;
        every_state(|g, now, path| {
            states += 1;
            let mut g = g.clone();
            let first = g.request(now + 1);
            if first == Request::Close {
                return;
            }
            assert_eq!(
                g.request(now + 1 + PATIENCE_MS),
                Request::Close,
                "after {path:?}"
            );
        });
        assert!(states > 10_000, "only {states} states visited");
    }

    #[test]
    fn a_clean_model_always_closes_on_the_first_request() {
        every_state(|g, now, path| {
            let mut g = g.clone();
            g.say(Say::Saved);
            assert_eq!(g.request(now), Request::Close, "after {path:?}");
        });
    }

    #[test]
    fn one_question_at_a_time() {
        // However the page got here, a request that asks is followed by no second question
        // before the page answers or the patience runs out.
        every_state(|g, now, path| {
            let mut g = g.clone();
            if g.request(now) != Request::Ask {
                return;
            }
            for dt in [0, 1, PATIENCE_MS / 2, PATIENCE_MS - 1] {
                assert_eq!(g.request(now + dt), Request::Wait, "after {path:?} +{dt}");
            }
        });
    }
}

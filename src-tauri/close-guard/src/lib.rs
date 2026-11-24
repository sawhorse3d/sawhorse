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
//! The page's answers: [`Say::Keep`] (Cancel: the window and the model stay), [`Say::Close`]
//! (Discard Changes, or Save once the model is saved: the shell destroys the window), and
//! [`Say::Exit`] (File > Exit), which the shell turns into an ordinary close request, so Exit
//! takes the X's path exactly.
//!
//! Since #120 the question has a Save answer, and a save can put a file dialog up for longer
//! than [`PATIENCE_MS`]. A person choosing a file name is not a page that has stopped
//! answering, so from the click on Save until the save has ended the page says
//! [`Say::Saving`] several times a second, and the rule above reads: a question out, and
//! [`PATIENCE_MS`] or longer since it went out **and** since the last [`Say::Saving`]: the
//! window closes. Each word buys at most [`PATIENCE_MS`] from when it is heard, and only
//! while a question is out; a page that stops saying it, for any reason, is the page of the
//! rule above, so two close requests two seconds apart with no [`Say::Saving`] between them
//! still always close the window.

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
    /// The person answered Cancel, or a save they asked for was cancelled or failed: keep
    /// the window and the model.
    Keep,
    /// The person answered Discard Changes, or Save and the model is saved (or nothing was
    /// unsaved by the time the page looked): close.
    Close,
    /// A save the person asked for at the question is running, its file dialog included
    /// (#120): the page is alive, wait for it.
    Saving,
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
            "saving" => Say::Saving,
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
    /// Destroy the window (no further close request: the person answered).
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
    /// When the page last said [`Say::Saving`] with that question out.
    saving_at: Option<u64>,
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
            Some(asked) => {
                // The page was last known to be alive when it was asked, or when it last said
                // it was saving, whichever is later.
                let heard = self.saving_at.map_or(asked, |s| s.max(asked));
                if now_ms.saturating_sub(heard) < PATIENCE_MS {
                    Request::Wait
                } else {
                    Request::Close
                }
            }
        }
    }

    /// The page says something at `now_ms` (the clock [`Guard::request`] reads).
    pub fn say(&mut self, say: Say, now_ms: u64) -> Then {
        match say {
            Say::Unsaved => self.unsaved = true,
            Say::Saved => {
                self.unsaved = false;
                self.answered();
                self.gave_up = false;
            }
            Say::Keep => self.answered(),
            Say::Close => {
                self.answered();
                return Then::Destroy;
            }
            // Only a question that is out can be waited for: with none out there is nothing
            // to hold, and the word is not kept for a later one.
            Say::Saving => {
                if self.asked_at.is_some() {
                    self.saving_at = Some(now_ms);
                }
            }
            Say::Failed => {
                self.answered();
                self.gave_up = true;
            }
            Say::Exit => return Then::RequestClose,
        }
        Then::Nothing
    }

    /// The question is no longer out.
    fn answered(&mut self) {
        self.asked_at = None;
        self.saving_at = None;
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
        assert_eq!(g.say(Say::Unsaved, 0), Then::Nothing);
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
        g.say(Say::Saved, 5);
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
        assert_eq!(g.say(Say::Keep, 900), Then::Nothing);
        assert!(!g.asking());
        // Still unsaved: asked again, even long after (the answer was a person's).
        assert_eq!(g.request(60_000), Request::Ask);
    }

    #[test]
    fn yes_destroys_the_window() {
        let mut g = unsaved();
        assert_eq!(g.request(0), Request::Ask);
        assert_eq!(g.say(Say::Close, 900), Then::Destroy);
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
        assert_eq!(g.say(Say::Failed, 1), Then::Nothing);
        assert!(!g.asking());
        assert_eq!(g.request(1), Request::Close);
    }

    #[test]
    fn saving_after_a_failed_question_asks_again_next_time() {
        let mut g = unsaved();
        g.request(0);
        g.say(Say::Failed, 1);
        g.say(Say::Saved, 2);
        g.say(Say::Unsaved, 3);
        assert_eq!(g.request(10), Request::Ask);
    }

    #[test]
    fn saving_clears_a_question_left_out() {
        // A question out, then the model saved and changed again much later: the old question
        // must not make the next request close without asking.
        let mut g = unsaved();
        assert_eq!(g.request(0), Request::Ask);
        g.say(Say::Saved, 1);
        g.say(Say::Unsaved, 2);
        assert_eq!(g.request(10 * PATIENCE_MS), Request::Ask);
    }

    #[test]
    fn exit_is_an_ordinary_close_request() {
        let mut g = unsaved();
        assert_eq!(g.say(Say::Exit, 7), Then::RequestClose);
        // Saying Exit changes nothing by itself: the request it causes is decided as the X's.
        assert_eq!(g, unsaved());
        let mut clean = Guard::new();
        assert_eq!(clean.say(Say::Exit, 7), Then::RequestClose);
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
            ("saving", Say::Saving),
            ("failed", Say::Failed),
            ("exit", Say::Exit),
        ] {
            assert_eq!(Say::parse(w), Some(s));
        }
        for w in [
            "", "Close", "yes", "no", "destroy", "close ", "asking", "Saving", "save",
        ] {
            assert_eq!(Say::parse(w), None, "{w:?}");
        }
    }

    #[derive(Clone, Copy)]
    enum Step {
        /// The page says a word now.
        Say(Say),
        /// The page says it is saving, this long after the step before.
        Saving(u64),
        /// A close request, this long after the step before.
        Request(u64),
    }

    /// Everything the page could say before #120 short of closing the window, and close
    /// requests at a few spacings.
    const STEPS: [Step; 8] = [
        Step::Say(Say::Unsaved),
        Step::Say(Say::Saved),
        Step::Say(Say::Keep),
        Step::Say(Say::Failed),
        Step::Request(0),
        Step::Request(1),
        Step::Request(PATIENCE_MS - 1),
        Step::Request(PATIENCE_MS),
    ];

    /// The same with the word of #120: "saving", at once, a little later, and as late as it
    /// can still count.
    const STEPS_SAVING: [Step; 11] = [
        Step::Say(Say::Unsaved),
        Step::Say(Say::Saved),
        Step::Say(Say::Keep),
        Step::Say(Say::Failed),
        Step::Saving(0),
        Step::Saving(250),
        Step::Saving(PATIENCE_MS - 1),
        Step::Request(0),
        Step::Request(1),
        Step::Request(PATIENCE_MS - 1),
        Step::Request(PATIENCE_MS),
    ];

    /// Every sequence of up to `depth` of `steps`, from a fresh guard.
    fn every_state(steps: &[Step], depth: usize, mut visit: impl FnMut(&Guard, u64, &[String])) {
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
                        next.say(say, t);
                        path.push(format!("{say:?}"));
                    }
                    Step::Saving(dt) => {
                        t += dt;
                        next.say(Say::Saving, t);
                        path.push(format!("Saving@{t}"));
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
        walk(&Guard::new(), 0, &mut Vec::new(), depth, steps, &mut visit);
    }

    #[test]
    fn never_trapped_two_requests_two_seconds_apart_always_close() {
        let mut states = 0;
        every_state(&STEPS, 6, |g, now, path| {
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

    /// The two seconds as a number, not through `PATIENCE_MS`: a patience changed by mistake
    /// (20 s, say) fails here, while every test that reads the constant would follow it.
    #[test]
    fn the_patience_is_two_seconds() {
        let mut g = unsaved();
        assert_eq!(g.request(10_000), Request::Ask);
        assert_eq!(g.request(11_900), Request::Wait, "1.9 s after the question");
        assert_eq!(
            g.request(12_100),
            Request::Close,
            "2.1 s after the question"
        );

        let mut g = unsaved();
        assert_eq!(g.request(10_000), Request::Ask);
        g.say(Say::Saving, 10_500);
        assert_eq!(
            g.request(12_400),
            Request::Wait,
            "1.9 s after the last saving"
        );
        assert_eq!(
            g.request(12_600),
            Request::Close,
            "2.1 s after the last saving"
        );
    }

    #[test]
    fn a_clean_model_always_closes_on_the_first_request() {
        every_state(&STEPS_SAVING, 5, |g, now, path| {
            let mut g = g.clone();
            g.say(Say::Saved, now);
            assert_eq!(g.request(now), Request::Close, "after {path:?}");
        });
    }

    #[test]
    fn one_question_at_a_time() {
        // However the page got here, a request that asks is followed by no second question
        // before the page answers or the patience runs out.
        every_state(&STEPS_SAVING, 5, |g, now, path| {
            let mut g = g.clone();
            if g.request(now) != Request::Ask {
                return;
            }
            for dt in [0, 1, PATIENCE_MS / 2, PATIENCE_MS - 1] {
                assert_eq!(g.request(now + dt), Request::Wait, "after {path:?} +{dt}");
            }
        });
    }

    // ---- #120: the Save answer, and the word "saving" --------------------------------------

    #[test]
    fn saving_keeps_the_window_while_the_word_keeps_coming() {
        // A person choosing a file name for a minute: a word every 250 ms, and close requests
        // (a second click on the X, the logout asking again) all through it.
        let mut g = unsaved();
        assert_eq!(g.request(1_000), Request::Ask);
        let mut t = 1_000;
        while t < 61_000 {
            t += 250;
            assert_eq!(g.say(Say::Saving, t), Then::Nothing);
            assert_eq!(g.request(t + 100), Request::Wait, "at {t} ms");
        }
        assert!(g.asking());
    }

    #[test]
    fn a_word_as_slow_as_a_throttled_timer_still_keeps_the_window() {
        // A timer slowed to once a second (a covered window) is still inside the patience.
        let mut g = unsaved();
        assert_eq!(g.request(0), Request::Ask);
        for t in (1_000..30_000).step_by(1_000) {
            g.say(Say::Saving, t);
            assert_eq!(g.request(t + 999), Request::Wait, "at {t} ms");
        }
    }

    #[test]
    fn the_word_stopping_closes_on_the_next_request_two_seconds_later() {
        // The page stopped (frozen, crashed) in the middle of a save.
        let mut g = unsaved();
        assert_eq!(g.request(0), Request::Ask);
        g.say(Say::Saving, 500);
        g.say(Say::Saving, 750);
        assert_eq!(g.request(750 + PATIENCE_MS - 1), Request::Wait);
        assert_eq!(g.request(750 + PATIENCE_MS), Request::Close);
    }

    #[test]
    fn saving_buys_time_from_the_word_not_from_the_request() {
        // A waiting request does not restart the patience, with the word as without it.
        let mut g = unsaved();
        assert_eq!(g.request(0), Request::Ask);
        g.say(Say::Saving, 1_000);
        for t in (1_100..1_000 + PATIENCE_MS).step_by(100) {
            assert_eq!(g.request(t), Request::Wait, "at {t} ms");
        }
        assert_eq!(g.request(1_000 + PATIENCE_MS), Request::Close);
    }

    #[test]
    fn saving_with_no_question_out_is_not_kept_for_a_later_one() {
        // Save chosen at File > New's question, then the X: the words before the request
        // count for nothing, the request asks, and the words after it hold the window.
        let mut g = unsaved();
        g.say(Say::Saving, 100);
        g.say(Say::Saving, 350);
        assert_eq!(g, unsaved());
        assert_eq!(g.request(400), Request::Ask);
        assert_eq!(g.request(400 + PATIENCE_MS), Request::Close);

        let mut g = unsaved();
        g.say(Say::Saving, 350);
        assert_eq!(g.request(400), Request::Ask);
        g.say(Say::Saving, 600);
        assert_eq!(g.request(400 + PATIENCE_MS), Request::Wait);
        assert_eq!(g.request(600 + PATIENCE_MS), Request::Close);
    }

    #[test]
    fn saving_does_not_answer_the_question_or_ask_another() {
        let mut g = unsaved();
        assert_eq!(g.request(0), Request::Ask);
        assert_eq!(g.say(Say::Saving, 100), Then::Nothing);
        assert!(g.asking());
        // The save ended: cancelled or failed is "keep", and the next request asks again.
        assert_eq!(g.say(Say::Keep, 5_000), Then::Nothing);
        assert!(!g.asking());
        assert_eq!(g.request(5_001), Request::Ask);
        // The word of the old question does not hold the new one.
        assert_eq!(g.request(5_001 + PATIENCE_MS), Request::Close);
    }

    #[test]
    fn a_save_that_worked_closes_whichever_word_comes_first() {
        // The page says "saved" (the model is clean) and "close" (the answer): either order.
        let mut g = unsaved();
        assert_eq!(g.request(0), Request::Ask);
        g.say(Say::Saving, 300);
        assert_eq!(g.say(Say::Saved, 9_000), Then::Nothing);
        assert_eq!(g.say(Say::Close, 9_001), Then::Destroy);

        let mut g = unsaved();
        assert_eq!(g.request(0), Request::Ask);
        g.say(Say::Saving, 300);
        assert_eq!(g.say(Say::Close, 9_000), Then::Destroy);
    }

    #[test]
    fn saving_does_not_bring_back_a_question_that_failed() {
        let mut g = unsaved();
        assert_eq!(g.request(0), Request::Ask);
        g.say(Say::Failed, 10);
        g.say(Say::Saving, 20);
        assert_eq!(g.request(30), Request::Close);
    }

    #[test]
    fn a_clock_that_steps_back_after_saving_waits_instead_of_closing() {
        let mut g = unsaved();
        assert_eq!(g.request(10_000), Request::Ask);
        g.say(Say::Saving, 10_500);
        assert_eq!(g.request(5), Request::Wait);
        // A word stamped before the question (the clock stepped back) never takes the wait
        // below the question's own.
        g.say(Say::Saving, 3);
        assert_eq!(g.request(10_000 + PATIENCE_MS - 1), Request::Wait);
        assert_eq!(g.request(10_000 + PATIENCE_MS), Request::Close);
    }

    /// The twin of `never_trapped_two_requests_two_seconds_apart_always_close` for the word
    /// of #120: however the page got there, "saving" at any spacing included, two requests
    /// two seconds apart with no "saving" between them always close.
    #[test]
    fn never_trapped_by_saving_two_requests_two_seconds_apart_with_no_word_between_always_close() {
        let mut states = 0;
        let mut waited = 0;
        every_state(&STEPS_SAVING, 6, |g, now, path| {
            states += 1;
            // The first request any time from now on: at once, and as the last word runs out.
            for dt in [0, 1, PATIENCE_MS - 1, PATIENCE_MS] {
                let mut g = g.clone();
                let first = g.request(now + dt);
                if first == Request::Close {
                    continue;
                }
                waited += 1;
                assert_eq!(
                    g.request(now + dt + PATIENCE_MS),
                    Request::Close,
                    "after {path:?}, first request at +{dt}"
                );
            }
        });
        assert!(states > 100_000, "only {states} states visited");
        assert!(
            waited > 100_000,
            "only {waited} first requests kept the window"
        );
    }

    /// The other side of it: the word is what holds the window, so the test above is not
    /// passing because "saving" does nothing.
    #[test]
    fn with_a_question_out_a_word_between_two_requests_two_seconds_apart_holds() {
        let mut held = 0;
        every_state(&STEPS_SAVING, 5, |g, now, path| {
            let mut g = g.clone();
            if g.request(now) == Request::Close {
                return;
            }
            g.say(Say::Saving, now + 1);
            assert_eq!(
                g.request(now + PATIENCE_MS),
                Request::Wait,
                "after {path:?}"
            );
            assert_eq!(
                g.request(now + 1 + PATIENCE_MS),
                Request::Close,
                "after {path:?}"
            );
            held += 1;
        });
        assert!(held > 10_000, "only {held} states held");
    }
}

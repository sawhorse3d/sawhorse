//! The text the desktop window's title bar shows (lane 39).
//!
//! The page names the model in its own title (`src/App.tsx`: a dot when there are unsaved
//! changes, the model's name or "Untitled", then " - Sawhorse"), and a browser shows that on
//! the tab. Before lane 39 the desktop app's title bar said only "Sawhorse", the title the
//! window is created with, whatever model was open; and since lane 37 a second double-click
//! on a `.shz` opens a second window, so two windows could not be told apart. Now the page
//! hands the same text to the shell's `window_title` command (`src-tauri/src/lib.rs`), which
//! passes it through [`title`] and sets it on the window that asked, and on no other.
//!
//! The text comes from the page, so [`title`] decides what of it reaches the title bar:
//!
//! - Control characters (tabs, line breaks, escape and the rest of Unicode's `Cc`) and the
//!   Unicode line and paragraph separators are removed: a title bar is one line.
//! - At most [`MAX_CHARS`] characters (Unicode scalar values, not bytes) are kept; a longer
//!   title is cut on a character boundary and ends in "…", so it is never split inside a
//!   character's bytes.
//! - A title that is empty, or only spaces, once that is done is [`FALLBACK`], "Sawhorse".
//!
//! Everything else, names in any script included, is kept as it is.

/// The title when the page's title has nothing to show: the one the window starts with.
pub const FALLBACK: &str = "Sawhorse";

/// The most characters the title bar is given. A model's name is at most a file name (255
/// characters on the common file systems) and title bars show far fewer, so a title longer
/// than this is cut.
pub const MAX_CHARS: usize = 200;

/// The mark at the end of a title that was cut.
pub const CUT: char = '…';

/// Whether `c` is kept in a title: not a control character and not a line or paragraph
/// separator.
fn kept(c: char) -> bool {
    !c.is_control() && c != '\u{2028}' && c != '\u{2029}'
}

/// The title to set on the window for the page's title `wanted`.
pub fn title(wanted: &str) -> String {
    let text: String = wanted.chars().filter(|&c| kept(c)).collect();
    if text.trim().is_empty() {
        return FALLBACK.to_string();
    }
    if text.chars().count() <= MAX_CHARS {
        return text;
    }
    let mut cut: String = text.chars().take(MAX_CHARS - 1).collect();
    cut.push(CUT);
    cut
}

#[cfg(test)]
mod tests {
    use super::*;

    fn len(s: &str) -> usize {
        s.chars().count()
    }

    #[test]
    fn the_pages_title_is_kept_as_it_is() {
        for t in [
            "Untitled - Sawhorse",
            "• Untitled - Sawhorse",
            "shelf - Sawhorse",
            "• a shelf for the hall - Sawhorse",
            "[draft] *? - Sawhorse",
            "  spaced  - Sawhorse",
        ] {
            assert_eq!(title(t), t);
        }
    }

    #[test]
    fn names_in_any_script_are_kept() {
        for t in [
            "Regal für Jörg - Sawhorse",
            "本棚 - Sawhorse",
            "• étagère - Sawhorse",
            "Полка - Sawhorse",
            "رف - Sawhorse",
            "🪚 bench - Sawhorse",
        ] {
            assert_eq!(title(t), t);
        }
    }

    #[test]
    fn control_characters_and_line_breaks_are_removed() {
        assert_eq!(title("shelf\n - Sawhorse"), "shelf - Sawhorse");
        assert_eq!(title("a\r\nb\tc - Sawhorse"), "abc - Sawhorse");
        assert_eq!(
            title("\u{1b}[31mred\u{1b}[0m - Sawhorse"),
            "[31mred[0m - Sawhorse"
        );
        assert_eq!(
            title("nul\0 bell\u{7} del\u{7f} - Sawhorse"),
            "nul bell del - Sawhorse"
        );
        assert_eq!(title("c1\u{85}\u{9b} - Sawhorse"), "c1 - Sawhorse");
        assert_eq!(
            title("line\u{2028}para\u{2029} - Sawhorse"),
            "linepara - Sawhorse"
        );
        for c in (0u32..0x20).chain(0x7f..0xa0).filter_map(char::from_u32) {
            let t = title(&format!("a{c}b"));
            assert_eq!(t, "ab", "U+{:04X}", c as u32);
        }
    }

    #[test]
    fn a_blank_title_is_sawhorse() {
        for t in [
            "", " ", "   ", "\n", "\t\r\n", " \u{1b} ", "\u{2028}", "\u{3000}",
        ] {
            assert_eq!(title(t), FALLBACK, "{t:?}");
        }
        assert_eq!(FALLBACK, "Sawhorse");
    }

    #[test]
    fn a_very_long_title_is_cut_on_a_character_boundary() {
        let long = "a".repeat(10_000);
        let t = title(&long);
        assert_eq!(len(&t), MAX_CHARS);
        assert!(t.ends_with(CUT));
        assert_eq!(&t[..t.len() - CUT.len_utf8()], "a".repeat(MAX_CHARS - 1));
        // Characters of several bytes each: counted as characters, never cut inside one.
        for c in ['é', '本', '🪚'] {
            let long: String = std::iter::repeat_n(c, MAX_CHARS + 50).collect();
            let t = title(&long);
            assert_eq!(len(&t), MAX_CHARS, "{c}");
            assert!(t.chars().take(MAX_CHARS - 1).all(|x| x == c), "{c}");
            assert!(t.ends_with(CUT), "{c}");
        }
        // Exactly the limit is not cut; one more is.
        let at_limit = "b".repeat(MAX_CHARS);
        assert_eq!(title(&at_limit), at_limit);
        let over = "b".repeat(MAX_CHARS + 1);
        assert_eq!(len(&title(&over)), MAX_CHARS);
        assert!(title(&over).ends_with(CUT));
        // The limit is counted after the control characters are removed.
        let padded = format!("{}{}", "\n".repeat(500), "c".repeat(MAX_CHARS));
        assert_eq!(title(&padded), "c".repeat(MAX_CHARS));
    }

    #[test]
    fn whatever_the_page_sends_the_answer_is_one_line_within_the_limit() {
        let samples = [
            "",
            "\n\n\n",
            "x",
            "• Untitled - Sawhorse",
            "a\u{0}b\u{1f}c\u{7f}d\u{9f}e\u{2028}f",
            "🪚",
        ];
        for s in samples.iter().map(|s| s.to_string()).chain([format!(
            "{}\n{}",
            "本".repeat(300),
            "é".repeat(300)
        )]) {
            let t = title(&s);
            assert!(!t.trim().is_empty(), "{s:?}");
            assert!(len(&t) <= MAX_CHARS, "{s:?}");
            assert!(t.chars().all(kept), "{s:?}");
        }
    }
}

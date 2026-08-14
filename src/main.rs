//! Claude Code status line: active model + current context size.
//!
//! Reads the status line payload from stdin and prints one line, e.g.
//!
//!     Opus 5 · 40k/1M
//!
//! Every field is optional: the payload's shape varies with session state and
//! Claude Code version, so anything missing degrades the line instead of
//! failing it. This never exits without printing something -- a blank status
//! line is the hardest kind of bug to diagnose from inside the footer.

use serde::Deserialize;
use std::io::{Read, Write};

#[derive(Deserialize, Default)]
#[serde(default)]
struct Payload {
    model: Option<Model>,
    context_window: Option<ContextWindow>,
}

#[derive(Deserialize)]
struct Model {
    display_name: Option<String>,
    id: Option<String>,
}

#[derive(Deserialize)]
struct ContextWindow {
    context_window_size: Option<u64>,
    /// Null before the first API call and right after /compact.
    current_usage: Option<Usage>,
}

#[derive(Deserialize)]
struct Usage {
    input_tokens: Option<u64>,
    cache_creation_input_tokens: Option<u64>,
    cache_read_input_tokens: Option<u64>,
}

// 256-colour foregrounds, named by role rather than hue. Chosen against a dark
// terminal; every one of them fails WCAG AA on a light background, so this is a
// dark-theme palette by construction.
const GREEN: u8 = 71; //  #5FAF5F  L* 64.9
const YELLOW: u8 = 179; // #D7AF5F  L* 73.5
/// Rose rather than a pure red (#D75F5F, 167). Under deuteranopia that red
/// simulates to #9A8F5C against GREEN's #A79B64 -- a dE2000 of 4, where the
/// just-noticeable difference is about 2.3, so "plenty of room" and "nearly
/// full" were the same olive smudge for roughly 6% of men. Rose keeps enough
/// blue to survive the collapse (dE2000 10.4) and still reads as an alarm.
const RED: u8 = 168; //    #D75F87  L* 56.5
const DIM: u8 = 244; //    #808080  L* 53.6
/// The model name is a label, not a signal: it changes at most once a session.
/// At the old #87AFD7 (110, L* 70.0) it was the brightest thing on the line,
/// louder than the number it sits next to and still louder than the alarm
/// colour at 90% full. Dropped below GREEN so colour on this line means one
/// thing only -- context pressure. Kept blue, and kept clear of DIM (dE2000
/// 23.0) so the name does not merge into the separator.
const NAME: u8 = 68; //    #5F87D7  L* 56.7

fn main() {
    let mut raw = String::new();
    let _ = std::io::stdin().read_to_string(&mut raw);

    let payload: Payload = serde_json::from_str(&raw).unwrap_or_default();
    // no-color.org: honoured when the variable is present *and* non-empty.
    // NO_COLOR= (set but blank) is the documented way to opt back in.
    let color = std::env::var_os("NO_COLOR").is_none_or(|v| v.is_empty());

    // Claude Code cancels an in-flight status line when a new update arrives,
    // which closes this pipe mid-write. println! panics on a write error, and
    // with panic = "abort" that turns a cancelled run into a crash. Discard the
    // error instead: being cancelled is routine here, not exceptional.
    let mut out = std::io::stdout().lock();
    let _ = writeln!(out, "{}", render(&payload, color));
}

/// The whole output format, in one place so the tests can assert on it.
fn render(payload: &Payload, color: bool) -> String {
    let model = payload
        .model
        .as_ref()
        .and_then(model_name)
        .unwrap_or_else(|| "model?".to_string());

    let cw = payload.context_window.as_ref();
    let size = cw.and_then(|c| c.context_window_size).unwrap_or(0);
    let used = cw.and_then(|c| c.current_usage.as_ref()).map(|u| {
        // What is resident in the window. output_tokens is deliberately
        // excluded: it is the last response's size, not context occupancy.
        u.input_tokens.unwrap_or(0)
            + u.cache_creation_input_tokens.unwrap_or(0)
            + u.cache_read_input_tokens.unwrap_or(0)
    });

    // Both halves are abbreviated so the fraction reads without counting
    // digits, and so the line barely changes width as the count climbs -- a
    // status line that twitches mid-session is worse than one that rounds.
    let ctx = match (used, size) {
        (Some(u), s) if s > 0 => {
            let pct = (u as f64 / s as f64) * 100.0;
            let (c, strong) = pressure(pct);
            paint_styled(&format!("{}/{}", abbrev(u), abbrev(s)), c, strong, color)
        }
        // A count with no window size to divide by: pressure is unknown, so it
        // gets the neutral colour. Green here would claim "plenty of room" on
        // no evidence, and it claimed it loudest at 950k.
        (Some(u), _) => paint(&abbrev(u), DIM, color),
        (None, s) if s > 0 => paint(&format!("—/{}", abbrev(s)), DIM, color),
        (None, _) => paint("—", DIM, color),
    };

    // The model name carries a space of its own now, so the dim dot is the
    // only gap on the line and each side stays one visual token.
    format!(
        "{} {} {}",
        paint(&model, NAME, color),
        paint("·", DIM, color),
        ctx
    )
}

/// The display name as given, minus any context-window marker; falls back to
/// the raw id with its vendor prefix cut. Never yields a blank or padded name.
fn model_name(m: &Model) -> Option<String> {
    let source = m
        .display_name
        .as_deref()
        .filter(|s| !s.trim().is_empty())
        .or_else(|| {
            m.id.as_deref()
                .filter(|s| !s.trim().is_empty())
                // strip_prefix, not trim_start_matches: the latter strips the
                // prefix repeatedly, so "claude-claude-x" would lose both.
                .map(|id| id.strip_prefix("claude-").unwrap_or(id))
        })?;

    let cut = strip_window(source);
    // A name that is nothing but a window marker keeps its marker. Better an
    // odd label than an empty one -- a blank here leaves the line opening with
    // a stray separator, which reads as a rendering fault rather than a name.
    Some(if cut.is_empty() { source.trim() } else { cut }.to_string())
}

/// Cuts a context-window marker off a model name: "Opus 5 (1M context)" is
/// "Opus 5", "opus-5[1m]" is "opus-5". The window size is the other half of this
/// line, so carrying it in the name too says the same thing twice. Matching on
/// the bracket rather than on known model names means new models need no change
/// here.
///
/// Returns empty when nothing precedes the marker; the caller decides what to
/// show instead.
fn strip_window(s: &str) -> &str {
    match s.find(['(', '[']) {
        None => s.trim(),
        Some(i) => s[..i].trim(),
    }
}

/// Colour and weight for a fill level.
///
/// The top tier is bold as well as coloured. Hue alone cannot separate three
/// states for every reader: whatever red is picked, either green and red
/// collapse under deuteranopia or green and yellow collapse under protanopia --
/// a search of the 256-colour cube finds no green/amber/red triple that is
/// separable under both, contrasts on a dark terminal, and still reads as an
/// alarm. Weight is a channel that survives all of it, plus greyscale and
/// screenshots.
fn pressure(pct: f64) -> (u8, bool) {
    if pct >= 85.0 {
        (RED, true)
    } else if pct >= 60.0 {
        (YELLOW, false)
    } else {
        (GREEN, false)
    }
}

/// 40238 -> "40k", 200000 -> "200k", 1000000 -> "1M".
fn abbrev(n: u64) -> String {
    // The bound is 999,500 rather than 1,000,000 so that a value which would
    // round up to "1000k" promotes to "1.0M" instead. Reachable now that token
    // counts come through here too, not just round window sizes.
    if n >= 999_500 {
        if n % 1_000_000 == 0 {
            format!("{}M", n / 1_000_000)
        } else {
            format!("{:.1}M", n as f64 / 1e6)
        }
    } else if n >= 1_000 {
        format!("{:.0}k", n as f64 / 1e3)
    } else {
        n.to_string()
    }
}

fn paint(s: &str, c: u8, on: bool) -> String {
    paint_styled(s, c, false, on)
}

/// Bold and colour go in one SGR sequence so the single reset closes both.
fn paint_styled(s: &str, c: u8, bold: bool, on: bool) -> String {
    if on {
        format!("\x1b[{}38;5;{}m{}\x1b[0m", if bold { "1;" } else { "" }, c, s)
    } else {
        s.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_numbers() {
        assert_eq!(abbrev(1_000_000), "1M");
        assert_eq!(abbrev(200_000), "200k");
        assert_eq!(abbrev(40_238), "40k");
        assert_eq!(abbrev(999), "999");
        assert_eq!(abbrev(0), "0");
        assert_eq!(abbrev(1_500_000), "1.5M");
        // The carry: 999,600 must not print as "1000k".
        assert_eq!(abbrev(999_000), "999k");
        assert_eq!(abbrev(999_600), "1.0M");
    }

    #[test]
    fn thresholds() {
        assert_eq!(pressure(10.0), (GREEN, false));
        assert_eq!(pressure(59.9), (GREEN, false));
        assert_eq!(pressure(60.0), (YELLOW, false));
        assert_eq!(pressure(84.9), (YELLOW, false));
        assert_eq!(pressure(85.0), (RED, true));
        assert_eq!(pressure(100.0), (RED, true));
    }

    /// Weight is the only pressure cue that survives colour vision deficiency,
    /// greyscale and screenshots, so the alarm tier must carry it.
    #[test]
    fn only_the_alarm_tier_is_bold() {
        assert!(!paint_styled("x", GREEN, false, true).contains("\x1b[1;"));
        assert!(paint_styled("x", RED, true, true).starts_with("\x1b[1;38;5;168m"));
        // NO_COLOR drops the weight along with the colour.
        assert_eq!(paint_styled("x", RED, true, false), "x");
    }

    #[test]
    fn tolerates_empty_and_garbage() {
        assert!(serde_json::from_str::<Payload>("{}").is_ok());
        assert!(serde_json::from_str::<Payload>("not json").is_err());
        // Unknown/extra fields in the real payload must not break parsing.
        let full = r#"{"model":{"id":"claude-opus-5","display_name":"Opus 5"},
            "cost":{"total_cost_usd":1.0},"fast_mode":false,"exceeds_200k_tokens":true}"#;
        let p: Payload = serde_json::from_str(full).unwrap();
        assert_eq!(model_name(p.model.as_ref().unwrap()).unwrap(), "Opus 5");
    }

    #[test]
    fn drops_the_context_window_marker() {
        let named = |d: &str, id: &str| {
            model_name(&Model {
                display_name: (!d.is_empty()).then(|| d.to_string()),
                id: Some(id.to_string()),
            })
            .unwrap()
        };
        assert_eq!(named("Opus 5 (1M context)", ""), "Opus 5");
        assert_eq!(named("", "claude-opus-5[1m]"), "opus-5");
        assert_eq!(named("Opus 5", "claude-opus-5"), "Opus 5");
        // Nothing left after the cut: keep the name rather than print blank.
        assert_eq!(named("(1M context)", ""), "(1M context)");
    }

    /// Every path out of model_name has to produce something visible. A blank
    /// name leaves the line opening with a stray separator, which reads as a
    /// rendering fault rather than as a name.
    #[test]
    fn never_yields_a_blank_name() {
        let named = |d: &str, id: &str| {
            model_name(&Model {
                display_name: Some(d.to_string()),
                id: Some(id.to_string()),
            })
        };
        // Whitespace before the marker used to cut down to "", because the
        // guard only caught a bracket at index 0.
        assert_eq!(named(" (1M context)", "").unwrap(), "(1M context)");
        assert_eq!(named("\t[1m]", "").unwrap(), "[1m]");
        // Whitespace-only display_name falls through to the id.
        assert_eq!(named("   ", "claude-opus-5").unwrap(), "opus-5");
        // Padding never survives into the line.
        assert_eq!(named("  Opus 5  ", "").unwrap(), "Opus 5");
        // Nothing usable anywhere: the caller's "model?" placeholder takes over.
        assert_eq!(named("  ", "   "), None);
        assert_eq!(model_name(&Model { display_name: None, id: None }), None);
    }

    #[test]
    fn strips_the_vendor_prefix_once() {
        let by_id = |id: &str| {
            model_name(&Model { display_name: None, id: Some(id.to_string()) }).unwrap()
        };
        assert_eq!(by_id("claude-opus-5"), "opus-5");
        // trim_start_matches would have eaten both and returned "opus-5".
        assert_eq!(by_id("claude-claude-opus-5"), "claude-opus-5");
        assert_eq!(by_id("gpt-4"), "gpt-4");
    }

    /// Pressure needs a denominator. Without one, the count is shown in the
    /// neutral colour rather than green, which would assert "plenty of room".
    #[test]
    fn no_window_size_means_no_pressure_colour() {
        let p: Payload = serde_json::from_str(
            r#"{"model":{"display_name":"Opus 5"},
               "context_window":{"current_usage":{"input_tokens":950000}}}"#,
        )
        .unwrap();
        assert_eq!(render(&p, false), "Opus 5 · 950k");
        let colored = render(&p, true);
        assert!(colored.contains(&format!("\x1b[38;5;{}m950k", DIM)));
        assert!(!colored.contains(&format!("\x1b[38;5;{}m950k", GREEN)));
    }

    /// The output format itself. NO_COLOR-style plain text, so the assertions
    /// are on the layout rather than on escape sequences.
    #[test]
    fn renders_the_line() {
        let line = |json: &str| {
            let p: Payload = serde_json::from_str(json).unwrap_or_default();
            render(&p, false)
        };

        assert_eq!(
            line(
                r#"{"model":{"display_name":"Opus 5 (1M context)"},
                   "context_window":{"context_window_size":1000000,
                     "current_usage":{"input_tokens":2,
                       "cache_creation_input_tokens":1197,
                       "cache_read_input_tokens":39039}}}"#
            ),
            "Opus 5 · 40k/1M"
        );

        // Fresh session or just after /compact: current_usage is null.
        assert_eq!(
            line(r#"{"model":{"display_name":"Opus 5"},"context_window":{"context_window_size":200000}}"#),
            "Opus 5 · —/200k"
        );

        // No display_name: the raw id, vendor prefix cut, left as it is.
        assert_eq!(
            line(
                r#"{"model":{"id":"claude-sonnet-5"},
                   "context_window":{"context_window_size":200000,
                     "current_usage":{"input_tokens":170005}}}"#
            ),
            "sonnet-5 · 170k/200k"
        );

        assert_eq!(line("{}"), "model? · —");
        assert_eq!(line("not json"), "model? · —");
    }

    #[test]
    fn color_wraps_each_segment() {
        let at = |used: u64| {
            let p: Payload = serde_json::from_str(&format!(
                r#"{{"model":{{"display_name":"Opus 5"}},"context_window":{{
                   "context_window_size":200000,"current_usage":{{"input_tokens":{}}}}}}}"#,
                used
            ))
            .unwrap();
            render(&p, true)
        };

        assert_eq!(
            at(40_238),
            "\x1b[38;5;68mOpus 5\x1b[0m \x1b[38;5;244m·\x1b[0m \x1b[38;5;71m40k/200k\x1b[0m"
        );
        // 90% -- the alarm tier picks up bold in the same escape sequence.
        assert_eq!(
            at(180_000),
            "\x1b[38;5;68mOpus 5\x1b[0m \x1b[38;5;244m·\x1b[0m \x1b[1;38;5;168m180k/200k\x1b[0m"
        );
    }
}

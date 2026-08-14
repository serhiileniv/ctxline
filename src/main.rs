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
use std::io::Read;

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

const GREEN: u8 = 71;
const YELLOW: u8 = 179;
const RED: u8 = 167;
const DIM: u8 = 244;
const BLUE: u8 = 110;

fn main() {
    let mut raw = String::new();
    let _ = std::io::stdin().read_to_string(&mut raw);

    let payload: Payload = serde_json::from_str(&raw).unwrap_or_default();
    let color = std::env::var_os("NO_COLOR").is_none();

    println!("{}", render(&payload, color));
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
            paint(&format!("{}/{}", abbrev(u), abbrev(s)), pressure(pct), color)
        }
        (Some(u), _) => paint(&abbrev(u), GREEN, color),
        (None, s) if s > 0 => paint(&format!("—/{}", abbrev(s)), DIM, color),
        (None, _) => paint("—", DIM, color),
    };

    // The model name carries a space of its own now, so the dim dot is the
    // only gap on the line and each side stays one visual token.
    format!(
        "{} {} {}",
        paint(&model, BLUE, color),
        paint("·", DIM, color),
        ctx
    )
}

/// The display name as given, minus any context-window marker; falls back to
/// the raw id with its vendor prefix cut.
fn model_name(m: &Model) -> Option<String> {
    if let Some(d) = m.display_name.as_ref().filter(|s| !s.is_empty()) {
        return Some(strip_window(d).to_string());
    }
    m.id
        .as_ref()
        .filter(|s| !s.is_empty())
        .map(|id| strip_window(id.trim_start_matches("claude-")).to_string())
}

/// Cuts a context-window marker off a model name: "Opus 5 (1M context)" is
/// "Opus 5", "opus-5[1m]" is "opus-5". The window size is the other half of this
/// line, so carrying it in the name too says the same thing twice. Matching on
/// the bracket rather than on known model names means new models need no change
/// here.
fn strip_window(s: &str) -> &str {
    match s.find(['(', '[']) {
        // A name that is nothing but a marker is left alone: better an odd
        // label than an empty one.
        Some(0) | None => s,
        Some(i) => s[..i].trim_end(),
    }
}

fn pressure(pct: f64) -> u8 {
    if pct >= 85.0 {
        RED
    } else if pct >= 60.0 {
        YELLOW
    } else {
        GREEN
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
    if on {
        format!("\x1b[38;5;{}m{}\x1b[0m", c, s)
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
        assert_eq!(pressure(10.0), GREEN);
        assert_eq!(pressure(60.0), YELLOW);
        assert_eq!(pressure(85.0), RED);
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
        let p: Payload = serde_json::from_str(
            r#"{"model":{"display_name":"Opus 5"},"context_window":{"context_window_size":200000,
               "current_usage":{"input_tokens":40238}}}"#,
        )
        .unwrap();
        assert_eq!(
            render(&p, true),
            "\x1b[38;5;110mOpus 5\x1b[0m \x1b[38;5;244m·\x1b[0m \x1b[38;5;71m40k/200k\x1b[0m"
        );
    }
}

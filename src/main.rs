//! Claude Code status line: active model + current context size.
//!
//! Reads the status line payload from stdin and prints one line, e.g.
//!
//!     Opus 5.5 · 40k/1M
//!
//! Every field is optional: the payload's shape varies with session state and
//! Claude Code version, so anything missing degrades the line instead of
//! failing it. This never exits without printing something -- a blank status
//! line is the hardest kind of bug to diagnose from inside the footer.

use serde::Deserialize;
use std::io::{Read, Write};
use serde_json::Value;
use std::path::{Path, PathBuf};

#[derive(Deserialize, Default)]
#[serde(default)]
struct Payload {
    model: Option<Model>,
    context_window: Option<ContextWindow>,
    workspace: Option<Workspace>,
    cwd: Option<String>,
}

#[derive(Deserialize)]
struct Workspace {
    project_dir: Option<String>,
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
    let window = window_arg(std::env::args().skip(1)).or_else(|| {
        let project = payload
            .workspace
            .as_ref()
            .and_then(|w| w.project_dir.as_deref())
            .or(payload.cwd.as_deref());
        let env = |k| std::env::var_os(k).filter(|v| !v.is_empty());
        let files = settings_files(
            project,
            env("CLAUDE_CONFIG_DIR").map(PathBuf::from),
            env("HOME").map(PathBuf::from),
        );
        let model = payload.model.as_ref().and_then(|m| m.id.as_deref());
        let compact = env("CLAUDE_CODE_AUTO_COMPACT_WINDOW");
        compaction_window(model, compact.as_ref().and_then(|v| v.to_str()), &files)
    });
    let _ = writeln!(out, "{}", render(&payload, window, color));
}

/// The whole output format, in one place so the tests can assert on it.
fn render(payload: &Payload, window: Option<u64>, color: bool) -> String {
    let model = payload
        .model
        .as_ref()
        .and_then(model_name)
        .unwrap_or_else(|| "model?".to_string());

    let cw = payload.context_window.as_ref();
    let size = cw.and_then(|c| c.context_window_size).unwrap_or(0);
    // The auto-compact window (--window, env or settings), which the payload does
    // not carry. Capped at the model's window, as Claude Code caps it.
    let size = match (window, size) {
        (Some(w), 0) => w,
        (Some(w), s) => w.min(s),
        (None, s) => s,
    };
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

/// Cuts a context-window marker off a model name: "Opus 5.5 (1M context)" is
/// "Opus 5.5", "opus-5-5[1m]" is "opus-5-5". The window size is the other half of this
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

/// `--window 400k` or `--window=400k`. A typo renders as no flag rather than
/// breaking the line.
fn window_arg(mut args: impl Iterator<Item = String>) -> Option<u64> {
    let value = loop {
        let a = args.next()?;
        if a == "--window" {
            break args.next()?;
        }
        if let Some(v) = a.strip_prefix("--window=") {
            break v.to_string();
        }
    };
    parse_window(&value)
}

/// A plain count or a k/M suffix, integers only; anything else is None.
fn parse_window(value: &str) -> Option<u64> {
    let (digits, scale) = match value.char_indices().last()? {
        (i, 'k' | 'K') => (&value[..i], 1_000),
        (i, 'm' | 'M') => (&value[..i], 1_000_000),
        _ => (value, 1),
    };
    digits
        .parse::<u64>()
        .ok()?
        .checked_mul(scale)
        .filter(|&n| n > 0)
}

#[cfg(target_os = "macos")]
const MANAGED: &str = "/Library/Application Support/ClaudeCode/managed-settings.json";
#[cfg(not(target_os = "macos"))]
const MANAGED: &str = "/etc/claude-code/managed-settings.json";

/// Settings files, highest precedence first. Server-managed settings, MDM
/// policies and `claude --settings` never reach a file here, so they can't be
/// seen; --window covers those by hand.
fn settings_files(
    project_dir: Option<&str>,
    config_dir: Option<PathBuf>,
    home: Option<PathBuf>,
) -> Vec<PathBuf> {
    let mut files = vec![PathBuf::from(MANAGED)];
    if let Some(p) = project_dir {
        let dir = Path::new(p).join(".claude");
        files.push(dir.join("settings.local.json"));
        files.push(dir.join("settings.json"));
    }
    if let Some(dir) = config_dir.or_else(|| home.map(|h| h.join(".claude"))) {
        files.push(dir.join("settings.json"));
    }
    files
}

/// The auto-compact window Claude Code would use for this model, or None for
/// the model's own window. Resolved the way Claude Code does it: the env var,
/// then the first file that sets a window -- its per-model entry before its
/// top-level key. "auto" is a setting too: it stops the search at the tuned
/// window rather than falling through to a lower file.
fn compaction_window(model_id: Option<&str>, env: Option<&str>, files: &[PathBuf]) -> Option<u64> {
    if let Some(n) = env.and_then(parse_window) {
        return Some(n);
    }
    let model = model_id.map(canonical);
    files
        .iter()
        .find_map(|f| {
            let v: Value = serde_json::from_str(&std::fs::read_to_string(f).ok()?).ok()?;
            let entry = model
                .and_then(|m| {
                    let all = v.get("modelSettings")?.as_object()?;
                    all.iter().find(|(k, _)| canonical(k) == m)
                })
                .and_then(|(_, e)| e.get("autoCompactWindow"));
            let found = [entry, v.get("autoCompactWindow")]
                .into_iter()
                .flatten()
                .find_map(window_setting);
            found
        })
        .flatten()
}

/// Some(Some(n)) for a window, Some(None) for "auto", None for unset or invalid.
fn window_setting(v: &Value) -> Option<Option<u64>> {
    match v {
        Value::String(s) if s == "auto" => Some(None),
        _ => v.as_u64().filter(|&n| n > 0).map(Some),
    }
}

/// The settings key for a model id: "us.anthropic.claude-opus-5-5-20260915-v1:0",
/// "claude-opus-5-5@20260915" and "claude-opus-5-5[1m]" are all
/// "claude-opus-5-5", as Claude Code matches them.
fn canonical(id: &str) -> &str {
    // The path first: a Bedrock ARN carries ':' ahead of its last '/'.
    let id = &id[id.rfind('/').map_or(0, |i| i + 1)..];
    let id = &id[..id.find(['[', '@', ':']).unwrap_or(id.len())];
    let id = &id[id.rfind('.').map_or(0, |i| i + 1)..];
    let digits = |t: &str| !t.is_empty() && t.bytes().all(|b| b.is_ascii_digit());
    let id = match id.rsplit_once('-') {
        Some((head, t)) if t.strip_prefix('v').is_some_and(digits) => head,
        _ => id,
    };
    match id.rsplit_once('-') {
        Some((head, t)) if t.len() == 8 && digits(t) => head,
        _ => id,
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
        let full = r#"{"model":{"id":"claude-opus-5-5","display_name":"Opus 5.5"},
            "cost":{"total_cost_usd":1.0},"fast_mode":false,"exceeds_200k_tokens":true}"#;
        let p: Payload = serde_json::from_str(full).unwrap();
        assert_eq!(model_name(p.model.as_ref().unwrap()).unwrap(), "Opus 5.5");
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
        assert_eq!(named("Opus 5.5 (1M context)", ""), "Opus 5.5");
        assert_eq!(named("", "claude-opus-5-5[1m]"), "opus-5-5");
        assert_eq!(named("Opus 5.5", "claude-opus-5-5"), "Opus 5.5");
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
        assert_eq!(named("   ", "claude-opus-5-5").unwrap(), "opus-5-5");
        // Padding never survives into the line.
        assert_eq!(named("  Opus 5.5  ", "").unwrap(), "Opus 5.5");
        // Nothing usable anywhere: the caller's "model?" placeholder takes over.
        assert_eq!(named("  ", "   "), None);
        assert_eq!(model_name(&Model { display_name: None, id: None }), None);
    }

    #[test]
    fn strips_the_vendor_prefix_once() {
        let by_id = |id: &str| {
            model_name(&Model { display_name: None, id: Some(id.to_string()) }).unwrap()
        };
        assert_eq!(by_id("claude-opus-5-5"), "opus-5-5");
        // trim_start_matches would have eaten both and returned "opus-5-5".
        assert_eq!(by_id("claude-claude-opus-5-5"), "claude-opus-5-5");
        assert_eq!(by_id("gpt-4"), "gpt-4");
    }

    /// Pressure needs a denominator. Without one, the count is shown in the
    /// neutral colour rather than green, which would assert "plenty of room".
    #[test]
    fn no_window_size_means_no_pressure_colour() {
        let p: Payload = serde_json::from_str(
            r#"{"model":{"display_name":"Opus 5.5"},
               "context_window":{"current_usage":{"input_tokens":950000}}}"#,
        )
        .unwrap();
        assert_eq!(render(&p, None, false), "Opus 5.5 · 950k");
        let colored = render(&p, None, true);
        assert!(colored.contains(&format!("\x1b[38;5;{}m950k", DIM)));
        assert!(!colored.contains(&format!("\x1b[38;5;{}m950k", GREEN)));
    }

    /// The output format itself. NO_COLOR-style plain text, so the assertions
    /// are on the layout rather than on escape sequences.
    #[test]
    fn renders_the_line() {
        let line = |json: &str| {
            let p: Payload = serde_json::from_str(json).unwrap_or_default();
            render(&p, None, false)
        };

        assert_eq!(
            line(
                r#"{"model":{"display_name":"Opus 5.5 (1M context)"},
                   "context_window":{"context_window_size":1000000,
                     "current_usage":{"input_tokens":2,
                       "cache_creation_input_tokens":1197,
                       "cache_read_input_tokens":39039}}}"#
            ),
            "Opus 5.5 · 40k/1M"
        );

        // Fresh session or just after /compact: current_usage is null.
        assert_eq!(
            line(r#"{"model":{"display_name":"Opus 5.5"},"context_window":{"context_window_size":200000}}"#),
            "Opus 5.5 · —/200k"
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
                r#"{{"model":{{"display_name":"Opus 5.5"}},"context_window":{{
                   "context_window_size":200000,"current_usage":{{"input_tokens":{}}}}}}}"#,
                used
            ))
            .unwrap();
            render(&p, None, true)
        };

        assert_eq!(
            at(40_238),
            "\x1b[38;5;68mOpus 5.5\x1b[0m \x1b[38;5;244m·\x1b[0m \x1b[38;5;71m40k/200k\x1b[0m"
        );
        // 90% -- the alarm tier picks up bold in the same escape sequence.
        assert_eq!(
            at(180_000),
            "\x1b[38;5;68mOpus 5.5\x1b[0m \x1b[38;5;244m·\x1b[0m \x1b[1;38;5;168m180k/200k\x1b[0m"
        );
    }

    fn usage(used: u64, size: Option<u64>) -> Payload {
        let size = size.map_or(String::new(), |s| format!(r#""context_window_size":{},"#, s));
        serde_json::from_str(&format!(
            r#"{{"model":{{"display_name":"Opus 5.5"}},"context_window":{{
               {}"current_usage":{{"input_tokens":{}}}}}}}"#,
            size, used
        ))
        .unwrap()
    }

    /// The case that motivated the flag: autoCompactWindow 400k on a 1M model.
    /// Against 1M this was a calm green 36% one step before compaction.
    #[test]
    fn window_sets_the_denominator() {
        let p = usage(364_000, Some(1_000_000));
        assert_eq!(render(&p, Some(400_000), false), "Opus 5.5 · 364k/400k");
        assert!(render(&p, Some(400_000), true).contains("\x1b[1;38;5;168m364k/400k"));
    }

    /// Claude Code caps the window at the model's own, so a 200k model keeps
    /// its 200k under a flag set for a 1M one.
    #[test]
    fn window_never_exceeds_the_model_window() {
        let p = usage(150_000, Some(200_000));
        assert_eq!(render(&p, Some(400_000), false), "Opus 5.5 · 150k/200k");
    }

    /// The flag is a denominator, so it brings pressure colour back too.
    #[test]
    fn window_without_payload_size() {
        let p = usage(364_000, None);
        assert_eq!(render(&p, Some(400_000), false), "Opus 5.5 · 364k/400k");
        assert!(render(&p, Some(400_000), true).contains("\x1b[1;38;5;168m364k/400k"));
    }

    fn args(a: &[&str]) -> Option<u64> {
        window_arg(a.iter().map(|s| s.to_string()))
    }

    #[test]
    fn parses_window_forms() {
        for a in [
            &["--window", "400000"][..],
            &["--window", "400k"],
            &["--window", "400K"],
            &["--window=400k"],
        ] {
            assert_eq!(args(a), Some(400_000), "{:?}", a);
        }
        assert_eq!(args(&["--window", "1M"]), Some(1_000_000));
        assert_eq!(args(&["--window=1m"]), Some(1_000_000));
    }

    /// A bad flag renders as no flag: the line must never break over its own
    /// configuration.
    #[test]
    fn bad_window_args_are_ignored() {
        for a in [
            &[][..],
            &["--window"],
            &["--window", "abc"],
            &["--window", "0"],
            &["--window", "1.5M"],
            &["--window", "k"],
            &["--window", "99999999999999999999k"],
            &["--foo"],
        ] {
            assert_eq!(args(a), None, "{:?}", a);
        }
    }

    /// A throwaway settings tree under the system temp dir, never ~/.claude.
    struct Tree(PathBuf);

    impl Tree {
        fn new(name: &str) -> Tree {
            let d = std::env::temp_dir().join(format!("ctxline-{}-{}", name, std::process::id()));
            let _ = std::fs::remove_dir_all(&d);
            std::fs::create_dir_all(&d).unwrap();
            Tree(d)
        }
        fn file(&self, name: &str, json: &str) -> PathBuf {
            let p = self.0.join(name);
            std::fs::write(&p, json).unwrap();
            p
        }
    }

    impl Drop for Tree {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    const MINE: &str = r#"{"modelSettings":{"claude-opus-5-5":{"autoCompactWindow":400000},
        "claude-fable-5-1":{"effortLevel":"xhigh"}}}"#;

    #[test]
    fn reads_the_window_per_model() {
        let t = Tree::new("per-model");
        let user = [t.file("user.json", MINE)];
        assert_eq!(compaction_window(Some("claude-opus-5-5"), None, &user), Some(400_000));
        assert_eq!(compaction_window(Some("claude-opus-5-5[1m]"), None, &user), Some(400_000));
        // Fable has an entry, but not for the window: nothing leaks across.
        assert_eq!(compaction_window(Some("claude-fable-5-1"), None, &user), None);
        assert_eq!(compaction_window(None, None, &user), None);
    }

    #[test]
    fn top_level_applies_without_a_model_entry() {
        let t = Tree::new("top-level");
        let f = [t.file("s.json", r#"{"autoCompactWindow":300000,
            "modelSettings":{"claude-opus-5-5":{"autoCompactWindow":400000}}}"#)];
        assert_eq!(compaction_window(Some("claude-fable-5-1"), None, &f), Some(300_000));
        assert_eq!(compaction_window(Some("claude-opus-5-5"), None, &f), Some(400_000));
    }

    #[test]
    fn higher_files_win_and_auto_stops_the_search() {
        let t = Tree::new("precedence");
        let high = t.file("high.json", r#"{"autoCompactWindow":250000}"#);
        let empty = t.file("empty.json", r#"{"theme":"light"}"#);
        let auto = t.file("auto.json", r#"{"autoCompactWindow":"auto"}"#);
        let low = t.file("low.json", MINE);
        let m = Some("claude-opus-5-5");
        assert_eq!(compaction_window(m, None, &[high.clone(), low.clone()]), Some(250_000));
        assert_eq!(compaction_window(m, None, &[empty.clone(), low.clone()]), Some(400_000));
        assert_eq!(compaction_window(m, None, &[auto, low.clone()]), None);
        // The env var beats every file.
        assert_eq!(compaction_window(m, Some("500k"), &[high, low]), Some(500_000));
    }

    #[test]
    fn broken_or_missing_files_are_skipped() {
        let t = Tree::new("broken");
        let low = t.file("low.json", MINE);
        let m = Some("claude-opus-5-5");
        let files = [
            t.0.join("missing.json"),
            t.file("garbage.json", "not json"),
            t.file("bad-value.json", r#"{"autoCompactWindow":"lots"}"#),
            t.file("zero.json", r#"{"autoCompactWindow":0}"#),
            t.file("wrong-shape.json", r#"{"modelSettings":[1,2]}"#),
            low,
        ];
        assert_eq!(compaction_window(m, None, &files), Some(400_000));
        assert_eq!(compaction_window(m, Some("nonsense"), &files), Some(400_000));
        assert_eq!(compaction_window(m, None, &[]), None);
    }

    #[test]
    fn canonical_ids() {
        for id in [
            "claude-opus-5-5",
            "claude-opus-5-5[1m]",
            "claude-opus-5-5-20260915",
            "us.anthropic.claude-opus-5-5-20260915-v1:0",
            "claude-opus-5-5@20260915",
            "arn:aws:bedrock:us-east-1:123456789012:inference-profile/us.anthropic.claude-opus-5-5-20260915-v1:0",
        ] {
            assert_eq!(canonical(id), "claude-opus-5-5", "{}", id);
        }
    }

    #[test]
    fn settings_files_in_precedence_order() {
        let home = PathBuf::from("/h");
        let f = settings_files(Some("/p"), None, Some(home.clone()));
        assert_eq!(f[1..], [
            PathBuf::from("/p/.claude/settings.local.json"),
            PathBuf::from("/p/.claude/settings.json"),
            PathBuf::from("/h/.claude/settings.json"),
        ]);
        assert!(f[0].ends_with("managed-settings.json"));
        // CLAUDE_CONFIG_DIR replaces ~/.claude; no project dir drops the project files.
        let f = settings_files(None, Some(PathBuf::from("/c")), Some(home));
        assert_eq!(f[1..], [PathBuf::from("/c/settings.json")]);
    }
}

//! Claude Code status line: active model + current context size.
//!
//! Reads the status line payload from stdin and prints one line, e.g.
//!
//!     opus-5  40,238 / 1M
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

    let ctx = match (used, size) {
        (Some(u), s) if s > 0 => {
            let pct = (u as f64 / s as f64) * 100.0;
            paint(&format!("{} / {}", commas(u), abbrev(s)), pressure(pct), color)
        }
        (Some(u), _) => paint(&commas(u), GREEN, color),
        (None, s) if s > 0 => paint(&format!("— / {}", abbrev(s)), DIM, color),
        (None, _) => paint("—", DIM, color),
    };

    println!("{}  {}", paint(&model, BLUE, color), ctx);
}

/// "Opus 5" -> "opus-5", falling back to the raw id with its vendor prefix cut.
fn model_name(m: &Model) -> Option<String> {
    if let Some(d) = m.display_name.as_ref().filter(|s| !s.is_empty()) {
        return Some(d.to_lowercase().replace(' ', "-"));
    }
    m.id
        .as_ref()
        .filter(|s| !s.is_empty())
        .map(|id| id.trim_start_matches("claude-").to_string())
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

/// 40238 -> "40,238". The stdlib has no locale formatter and this isn't worth a dependency.
fn commas(n: u64) -> String {
    let s = n.to_string();
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(s.len() + s.len() / 3);
    for (i, b) in bytes.iter().enumerate() {
        if i > 0 && (bytes.len() - i) % 3 == 0 {
            out.push(',');
        }
        out.push(*b as char);
    }
    out
}

/// Window sizes only: 200000 -> "200k", 1000000 -> "1M".
fn abbrev(n: u64) -> String {
    if n >= 1_000_000 {
        if n % 1_000_000 == 0 {
            format!("{}M", n / 1_000_000)
        } else {
            format!("{:.1}M", n as f64 / 1e6)
        }
    } else if n >= 1_000 {
        if n % 1_000 == 0 {
            format!("{}k", n / 1_000)
        } else {
            format!("{:.0}k", n as f64 / 1e3)
        }
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
        assert_eq!(commas(40238), "40,238");
        assert_eq!(commas(1000000), "1,000,000");
        assert_eq!(commas(42), "42");
        assert_eq!(abbrev(1_000_000), "1M");
        assert_eq!(abbrev(200_000), "200k");
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
        assert_eq!(model_name(p.model.as_ref().unwrap()).unwrap(), "opus-5");
    }
}

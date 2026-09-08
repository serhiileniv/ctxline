# ctxline

Model and context size in your [Claude Code](https://code.claude.com) status line. Nothing else.

![ctxline in a Claude Code footer: Opus 5 with the token count climbing from 8k to 961k of a 1M window, green through yellow to bold rose](assets/demo.gif)

```sh
curl -fsSL https://raw.githubusercontent.com/serhiileniv/ctxline/main/install.sh | sh
```

Restart Claude Code and it's there. One static binary, ~330KB, ~3ms a render — no config to write,
nothing to install alongside it, no transcript to parse.

[![support: monobank jar](https://img.shields.io/badge/support-monobank_jar-172B35)](https://send.monobank.ua/jar/3zo8nv9iuF)

## The line

```
Opus 5 · 40k/1M
```

The token count is colored by pressure — green normally, yellow past 60%, and bold rose past 85%. The
model name is deliberately the quietest thing on the line: it changes at most once a session, so color
here means one thing only, which is how full the window is.

## Why

Claude Code pipes a JSON payload to a command of your choosing on every assistant message and renders
its stdout in the footer. That payload already carries a `context_window` object, so this is a pure
`stdin → stdout` filter: no transcript parsing, no subprocesses, no I/O of its own.

It's written in Rust because Claude Code **cancels an in-flight status line script** when a new update
arrives, which makes startup time the one property that matters. The binary is ~330KB and runs in ~3ms.

## Install

```sh
curl -fsSL https://raw.githubusercontent.com/serhiileniv/ctxline/main/install.sh | sh
```

That downloads the binary for your machine into `~/.local/bin`, points
`~/.claude/settings.json` at it, and backs up that file first if it already existed. Restart Claude
Code afterwards — `settings.json` is read at startup.

macOS and Linux, Intel and ARM. The Linux binaries are static (musl), so distro doesn't matter. If
there's no prebuilt binary for your platform the script builds from source instead, which needs a Rust
toolchain ([rustup](https://rustup.rs)).

A few knobs, all optional:

```sh
CTXLINE_BIN_DIR=/usr/local/bin   # where the binary goes (default ~/.local/bin)
CTXLINE_VERSION=v0.1.0           # pin a release (default: latest)
CLAUDE_CONFIG_DIR=~/.claude      # which Claude Code config to wire up
```

Piping into `sh` means flags go through the shell: `... | sh -s -- --no-config` installs the binary and
prints the settings snippet instead of editing anything.

To undo it, restore the backup the installer named (`mv ~/.claude/settings.json.bak-<stamp>
~/.claude/settings.json`) and delete `~/.local/bin/ctxline`. To update, run the same one-liner again.

### From source

```sh
git clone https://github.com/serhiileniv/ctxline
cd ctxline
cargo build --release
```

Then point Claude Code at the binary in `~/.claude/settings.json`:

```json
{
  "statusLine": {
    "type": "command",
    "command": "/absolute/path/to/ctxline/target/release/ctxline",
    "padding": 0
  }
}
```

`padding: 0` starts the line flush at the left edge.

## What it reads

Only these fields of the [status line payload](https://code.claude.com/docs/en/statusline.md):

```jsonc
{
  "model": { "id": "claude-opus-5", "display_name": "Opus 5" },
  "context_window": {
    "context_window_size": 1000000,
    "current_usage": {                 // null before the first API call and right after /compact
      "input_tokens": 2,
      "cache_creation_input_tokens": 1197,
      "cache_read_input_tokens": 39039
    }
  }
}
```

Tokens in use = `input_tokens + cache_creation_input_tokens + cache_read_input_tokens`.
`output_tokens` is deliberately excluded: it's the last response's size, not context occupancy.

That's the last request as the API counted it, not an estimate, so it can read a little lower than the
uncached-token figure Claude Code prints on the other side of the same footer. That one estimates what
the *next* request will send with a cold cache: the last response plus everything added since the call
reported here. Neither is wrong — they're one turn apart.

The model name is whatever the payload says, as it says it — the same `Opus 5` that `/model` and
`/status` show. There's no list of known models and no reformatting, so a model released tomorrow needs
no change here. The one thing dropped from it is a context-window marker (`Opus 5 (1M context)`,
`claude-opus-5[1m]`), since the window size is already the right half of the line.

Both numbers are abbreviated to the same shape. `40k/1M` is a fraction you can read without counting
digits, and it barely changes width as the count climbs — a status line that twitches while you're
reading it is worse than one that rounds.

Every field is optional. The payload's shape varies with session state and Claude Code version, so
anything missing degrades the line rather than failing it — and it always prints *something*, because a
blank status line is the hardest kind of bug to diagnose from inside the footer.

| Situation | Output |
| --- | --- |
| Normal | `Opus 5 · 40k/1M` |
| Fresh session, or just after `/compact` | `Opus 5 · —/200k` |
| No `display_name`, falls back to `id` | `sonnet-5 · 62k/200k` |
| A 1M-context variant (`Opus 5 (1M context)`, `claude-opus-5[1m]`) | `Opus 5 · 40k/1M` |
| Tokens known but no window size | `Opus 5 · 950k` (neutral — pressure needs a denominator) |
| Empty or unparseable payload | `model? · —` |

Set `NO_COLOR` to any non-empty value to disable the ANSI escapes — bold goes with them. Per
[no-color.org](https://no-color.org) a variable that is set but empty does *not* disable color, so
`NO_COLOR=` is the way back to a colored line without unsetting anything.

Being cancelled is normal — Claude Code kills an in-flight status line when a new update arrives, which
closes the pipe mid-write. That is treated as routine rather than as an error worth crashing over.

## Colors

The palette is dark-terminal only. All five colors clear WCAG AA (4.5:1) on pure black; on a lighter
dark theme the two darker ones and the separator drop into large-text territory (3.0–4.5:1), which is
fine for a `·` that carries no information. On a light background nothing clears AA and the yellow is
effectively invisible at 1.9:1, so a light profile will wash the line out.

Past 85% the count goes **bold** as well as rose, and that is not decoration. Hue alone cannot separate
three states for every reader: pick a pure red and it collapses into green under deuteranopia
(ΔE2000 ≈ 4, against a just-noticeable difference of ~2.3); shift the green to fix that and it collapses
into the yellow under protanopia instead. Searching the whole 256-color cube turns up no green/amber/red
triple that survives both, contrasts on a dark terminal, and still reads as an alarm — the ones that
pass the math use a pale pink for "red". Weight is a channel that survives all of it, including
greyscale and screenshots. The rose (`168`) over a pure red (`167`) buys back most of the hue
separation on top.

## Tweaking

Everything lives in `src/main.rs`. The color constants and the `60.0` / `85.0` thresholds in
`pressure()` are near the top, each with the measurement behind it. Rebuild with
`cargo build --release` — no restart needed, Claude Code re-runs the binary on the next message.

## Testing

```sh
cargo test
cat fixtures/sample.json | ./target/release/ctxline
./demo.sh
```

`fixtures/sample.json` is a realistic payload; piping it in is the fastest way to check a change without
restarting anything.

`./demo.sh` renders every state the line can reach — both window sizes filling up, all three pressure
tiers, each degraded payload, and both `NO_COLOR` cases — as real terminal output. A format or palette
change looks fine on one sample and falls apart at the edges, so this is the check worth running before
committing one.

The GIF at the top is generated too, never hand-drawn: `scripts/gif.sh` feeds a climbing series of
payloads through the same binary and `vhs assets/demo.tape` records the terminal. Regenerate it after a
palette or format change — `brew install vhs`, then `vhs assets/demo.tape`.

## License

MIT

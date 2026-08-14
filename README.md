# ctxline

Model and context size in your [Claude Code](https://code.claude.com) status line. Nothing else.

```
Opus 5 · 40k/1M
```

The token count is colored by pressure — green normally, yellow past 60%, red past 85%.

## Why

Claude Code pipes a JSON payload to a command of your choosing on every assistant message and renders
its stdout in the footer. That payload already carries a `context_window` object, so this is a pure
`stdin → stdout` filter: no transcript parsing, no subprocesses, no I/O of its own.

It's written in Rust because Claude Code **cancels an in-flight status line script** when a new update
arrives, which makes startup time the one property that matters. The binary is ~330KB and runs in ~3ms.

## Install

Needs a Rust toolchain ([rustup](https://rustup.rs)).

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

`padding: 0` starts the line flush at the left edge. Restart Claude Code — `settings.json` is read at
startup.

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
| No `display_name`, falls back to `id` | `sonnet-5 · 170k/200k` |
| A 1M-context variant (`Opus 5 (1M context)`, `claude-opus-5[1m]`) | `Opus 5 · 40k/1M` |
| Empty or unparseable payload | `model? · —` |

Set `NO_COLOR` to disable the ANSI escapes.

## Tweaking

Everything lives in `src/main.rs`. The color constants and the `60.0` / `85.0` thresholds in
`pressure()` are near the top. Rebuild with `cargo build --release` — no restart needed, Claude Code
re-runs the binary on the next message.

## Testing

```sh
cargo test
cat fixtures/sample.json | ./target/release/ctxline
```

`fixtures/sample.json` is a realistic payload; piping it in is the fastest way to check a change without
restarting anything.

## License

MIT

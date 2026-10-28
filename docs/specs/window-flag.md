# Spec: auto-compact window as the denominator

## Goal
Count and colour track the auto-compact window in every session, with no per-install setup.
With `autoCompactWindow: 400000` on a 1M model, the line reads `364k/1M` in green while Claude Code
says `1% until auto-compact`. The payload doesn't carry the setting, so ctxline resolves it itself
the way Claude Code does.

## Non-goals
- **Subtracting Claude Code's compaction buffer**: about 33k, inferred from two screenshots
  (compaction at ~367k with a 400k window). It isn't documented and can change in any release.
- **Server-managed settings, MDM policies, `--settings`, and the `--autocompact` launch flag**:
  none of them is visible from a status line process. `--window` covers those cases by hand.
- **Caching across runs**: the files are small, so a cache adds state for no measurable gain.

## Current state
- `src/main.rs` already has `--window N` (`window_arg`). `render(&Payload, window, color)` uses
  `min(window, context_window_size)` as the denominator.
- Before this change ctxline did no file I/O and read only stdin.
- The payload has `model.id`, `workspace.project_dir` and `cwd`, but no auto-compact field.
- From the docs (model-config, settings-reference):
  - `CLAUDE_CODE_AUTO_COMPACT_WINDOW` env var beats every setting.
  - In each file, `modelSettings.<canonical id>.autoCompactWindow` beats top-level `autoCompactWindow`.
  - `"auto"` means the tuned window, i.e. no override.
  - File precedence: managed > project local > project > user.
  - Claude Code caps the window at the model's context window.

## Proposed design
Resolution order, first hit wins:

| # | Source | Where |
| --- | --- | --- |
| 1 | `--window N` | command line (manual override, kept) |
| 2 | `CLAUDE_CODE_AUTO_COMPACT_WINDOW` | env, inherited from Claude Code |
| 3 | managed settings | macOS `/Library/Application Support/ClaudeCode/managed-settings.json`, Linux `/etc/claude-code/managed-settings.json` |
| 4 | project local | `<project_dir>/.claude/settings.local.json` |
| 5 | project | `<project_dir>/.claude/settings.json` |
| 6 | user | `$CLAUDE_CONFIG_DIR/settings.json`, else `~/.claude/settings.json` |

- In each file, `modelSettings[<canonical>].autoCompactWindow` is checked first, then top-level
  `autoCompactWindow`. A file that sets either key decides, and the lower files are not read.
- `"auto"` is a hit that means no override: it stops the search and keeps the model window.
- **Canonical id**: `model.id` after its last `/` (Bedrock ARN), cut at `[`, `@` or `:` (`[1m]`,
  Vertex `@date`, Bedrock `:0`), the provider prefix up to the last `.` stripped, then a trailing
  `-vN` and `-YYYYMMDD` dropped.
  Matched against keys normalised the same way.
- `project_dir` comes from `workspace.project_dir`, else `cwd`. If neither is present, sources 4–5
  are skipped.
- Values: an integer token count. The env var and flag also take `k`/`M`, the same parser as
  `--window`. Anything else, a value of 0, or an unreadable or invalid file counts as unset and
  the search moves to the next source.
- The result goes into the existing `render(…, window, …)`: capped at `context_window_size`, with
  output shape and thresholds unchanged.
- Reads use `std::fs::read_to_string` and the existing serde_json, with no new dependency. The
  settings types stay minimal: `#[serde(default)]`, unknown keys ignored.
- **Cost**: at most 4 small reads, the user file is about 45KB here. Check: release render time stays
  under 10ms (today about 3ms).

## Acceptance criteria
- [ ] With user settings `modelSettings.claude-opus-5-5.autoCompactWindow: 400000` and no flag,
      a 364k/1M Opus payload prints `364k/400k` in bold rose.
- [ ] The same settings with a `claude-fable-5-1` payload print `/1M` (per-model, no leak).
- [ ] `model.id` `claude-opus-5-5[1m]` matches the `claude-opus-5-5` entry.
- [ ] Top-level `autoCompactWindow` applies when the model has no entry.
- [ ] Project local beats project, which beats user. Managed beats all three.
- [ ] `"auto"` in a higher file stops the search, so a lower file's value is ignored.
- [ ] The env var beats every file, and `--window` beats the env var.
- [ ] Missing, unreadable or garbage settings files render exactly like today.
- [ ] Tests run against temp dirs, never the real `~/.claude`.
- [ ] Release render time stays under 10ms with the real user settings (`hyperfine`, or 100 runs).
- [ ] README "Auto-compact window" section rewritten: automatic, the list of what it can't see,
      `--window` as the override.

## Decisions
1. Managed settings: only the single `managed-settings.json` is read, not the `managed-settings.d/`
   drop-ins or MDM plists. `--window` covers those.
2. README: "no I/O of its own" becomes "the only files it reads are Claude Code's own settings".

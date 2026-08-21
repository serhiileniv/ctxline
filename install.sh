#!/bin/sh
# ctxline installer.
#
#   curl -fsSL https://raw.githubusercontent.com/serhiileniv/ctxline/main/install.sh | sh
#
# Downloads the release binary for this machine, drops it in ~/.local/bin, and
# points Claude Code's status line at it. Falls back to building from source
# when there is no prebuilt binary for the platform but a Rust toolchain is
# present.
#
# Knobs, all optional:
#   CTXLINE_VERSION=v0.1.0     pin a release (default: latest)
#   CTXLINE_BIN_DIR=/usr/local/bin
#   CLAUDE_CONFIG_DIR=~/.claude
#   --no-config                install the binary, leave settings.json alone
set -eu

REPO="serhiileniv/ctxline"
VERSION="${CTXLINE_VERSION:-latest}"
BIN_DIR="${CTXLINE_BIN_DIR:-$HOME/.local/bin}"
# CLAUDE_CONFIG_DIR may hold a search path; Claude Code writes to the first entry.
CONFIG_DIR="${CLAUDE_CONFIG_DIR:-$HOME/.claude}"
CONFIG_DIR="${CONFIG_DIR%%:*}"
CONFIGURE=1

for arg in "$@"; do
    case "$arg" in
        --no-config) CONFIGURE=0 ;;
        -h|--help) sed -n '2,17p' "$0" | cut -c3-; exit 0 ;;
        *) echo "ctxline: unknown option: $arg" >&2; exit 2 ;;
    esac
done

say()  { printf '%s\n' "$*"; }
warn() { printf '! %s\n' "$*" >&2; }
die()  { printf 'ctxline: %s\n' "$*" >&2; exit 1; }
# Same, but from inside install_release's command substitution: exit 3 marks
# "the download arrived and something was wrong with it", which the caller must
# not paper over by falling back to a source build.
fatal() { printf 'ctxline: %s\n' "$*" >&2; exit 3; }
have() { command -v "$1" >/dev/null 2>&1; }

# --- which binary --------------------------------------------------------

target() {
    os="$(uname -s)"
    arch="$(uname -m)"
    case "$os-$arch" in
        Darwin-arm64)         echo aarch64-apple-darwin ;;
        Darwin-x86_64)        echo x86_64-apple-darwin ;;
        Linux-x86_64|Linux-amd64) echo x86_64-unknown-linux-musl ;;
        Linux-aarch64|Linux-arm64) echo aarch64-unknown-linux-musl ;;
        *) return 1 ;;
    esac
}

url_for() {
    if [ "$VERSION" = latest ]; then
        echo "https://github.com/$REPO/releases/latest/download/$1"
    else
        echo "https://github.com/$REPO/releases/download/$VERSION/$1"
    fi
}

fetch() { curl -fsSL --proto '=https' --tlsv1.2 -o "$2" "$1"; }

sha256_of() {
    if have sha256sum; then sha256sum "$1" | cut -d' ' -f1
    elif have shasum;   then shasum -a 256 "$1" | cut -d' ' -f1
    fi
}

# Prints the installed path on stdout; progress goes to stderr so the caller can
# capture one without the other. Exit 1 means "no such asset" (the caller may
# fall back to source); exit 3 means the asset was bad and nothing should follow.
install_release() {
    tgt="$1"
    asset="ctxline-$tgt.tar.gz"
    tmp="$(mktemp -d)"
    # shellcheck disable=SC2064  # expand $tmp now, not at trap time
    trap "rm -rf '$tmp'" EXIT INT TERM

    say "Downloading $asset ($VERSION)" >&2
    fetch "$(url_for "$asset")" "$tmp/$asset" || return 1

    if fetch "$(url_for checksums.txt)" "$tmp/checksums.txt" 2>/dev/null; then
        want="$(grep " $asset\$" "$tmp/checksums.txt" | cut -d' ' -f1 || true)"
        got="$(sha256_of "$tmp/$asset")"
        if [ -z "$got" ]; then
            warn "no sha256 tool found, skipping checksum verification"
        elif [ -z "$want" ]; then
            warn "$asset is not listed in checksums.txt, skipping verification"
        elif [ "$want" != "$got" ]; then
            fatal "checksum mismatch for $asset (expected $want, got $got)"
        fi
    else
        warn "could not fetch checksums.txt, skipping checksum verification"
    fi

    tar -xzf "$tmp/$asset" -C "$tmp" || fatal "could not unpack $asset"
    [ -f "$tmp/ctxline" ] || fatal "$asset does not contain a ctxline binary"

    mkdir -p "$BIN_DIR" || fatal "could not create $BIN_DIR"
    # Replace by rename so a running status line keeps its open file.
    mv -f "$tmp/ctxline" "$BIN_DIR/ctxline"
    chmod +x "$BIN_DIR/ctxline"
    echo "$BIN_DIR/ctxline"
}

install_from_source() {
    have cargo || return 1
    say "Building from source with cargo" >&2
    root="${CARGO_HOME:-$HOME/.cargo}"
    if [ "$VERSION" = latest ]; then
        cargo install --git "https://github.com/$REPO" --locked ctxline >&2
    else
        cargo install --git "https://github.com/$REPO" --tag "$VERSION" --locked ctxline >&2
    fi
    echo "$root/bin/ctxline"
}

# --- settings.json -------------------------------------------------------

# Rewrites settings.json in place, preserving every other key. Only ever writes
# through jq or python3: hand-rolled JSON editing in sh would corrupt files that
# already hold hooks, permissions and the rest.
configure() {
    bin="$1"
    settings="$CONFIG_DIR/settings.json"
    snippet='  "statusLine": { "type": "command", "command": "'"$bin"'", "padding": 0 }'

    mkdir -p "$CONFIG_DIR" || die "could not create $CONFIG_DIR"

    if [ ! -s "$settings" ]; then
        printf '{\n%s\n}\n' "$snippet" > "$settings"
        say "Wrote $settings"
        return
    fi

    if have jq; then
        jq empty "$settings" >/dev/null 2>&1 || die "$settings is not valid JSON; fix it or rerun with --no-config"
        previous="$(jq -r '.statusLine.command // empty' "$settings")"
        backup="$settings.bak-$(date +%Y%m%d%H%M%S)"
        cp "$settings" "$backup"
        jq --arg cmd "$bin" \
           '.statusLine = {type: "command", command: $cmd, padding: 0}' \
           "$settings" > "$settings.tmp" && mv "$settings.tmp" "$settings"
    elif have python3; then
        previous="$(python3 - "$settings" <<'PY'
import json, sys
try:
    with open(sys.argv[1]) as f:
        print(json.load(f).get("statusLine", {}).get("command", ""))
except Exception:
    sys.exit(3)
PY
)" || die "$settings is not valid JSON; fix it or rerun with --no-config"
        backup="$settings.bak-$(date +%Y%m%d%H%M%S)"
        cp "$settings" "$backup"
        python3 - "$settings" "$bin" <<'PY'
import json, sys
path, cmd = sys.argv[1], sys.argv[2]
with open(path) as f:
    data = json.load(f)
data["statusLine"] = {"type": "command", "command": cmd, "padding": 0}
with open(path, "w") as f:
    json.dump(data, f, indent=2)
    f.write("\n")
PY
    else
        warn "neither jq nor python3 found, so $settings was left untouched."
        warn "Add this to it by hand:"
        printf '\n%s\n\n' "$snippet" >&2
        return 1
    fi

    say "Updated $settings (backup: $backup)"
    if [ -n "$previous" ] && [ "${previous#*ctxline}" = "$previous" ]; then
        warn "replaced an existing status line: $previous"
        warn "to put it back: mv '$backup' '$settings'"
    fi
}

# --- run -----------------------------------------------------------------

have curl || die "curl is required"
have tar  || die "tar is required"

if tgt="$(target)"; then
    rc=0
    BIN="$(install_release "$tgt")" || rc=$?
    # 3: the asset was fetched and rejected. It has already said why.
    if [ "$rc" -eq 3 ]; then exit 1; fi
    if [ "$rc" -ne 0 ]; then
        warn "no prebuilt binary for $tgt at $VERSION"
        BIN="$(install_from_source)" || die "download failed and no cargo toolchain to build from source — install Rust from https://rustup.rs and retry"
    fi
else
    warn "unsupported platform: $(uname -s) $(uname -m)"
    BIN="$(install_from_source)" || die "no prebuilt binary for this platform and no cargo toolchain — install Rust from https://rustup.rs and retry"
fi

say "Installed $BIN"

if [ "$CONFIGURE" = 1 ]; then
    if configure "$BIN"; then
        say ""
        say "Restart Claude Code — settings.json is read at startup."
    fi
else
    say ""
    say "Add this to ${CONFIG_DIR}/settings.json:"
    say ""
    say '  "statusLine": { "type": "command", "command": "'"$BIN"'", "padding": 0 }'
fi

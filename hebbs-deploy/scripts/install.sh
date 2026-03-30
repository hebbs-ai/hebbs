#!/bin/sh
# HEBBS Installer
# Usage: curl -sSf https://hebbs.ai/install | sh
#
# Flags:
#   --full              - Install full engine binary (default: CLI-only for remote server use)
#   --with-systemd      - Install systemd unit file (Linux only, requires root)
#
# Environment variables:
#   HEBBS_VERSION       - Version to install (default: latest)
#   HEBBS_INSTALL_DIR   - Installation directory (default: /usr/local/bin or ~/.hebbs/bin)
#   HEBBS_REPO          - GitHub repository (default: hebbs-ai/hebbs)
#   HEBBS_NO_VERIFY     - Skip checksum verification if set to 1

set -eu

REPO="${HEBBS_REPO:-hebbs-ai/hebbs}"
BASE_URL="https://github.com/${REPO}/releases"
WITH_SYSTEMD=0
FULL_INSTALL=0

for arg in "$@"; do
    case "$arg" in
        --with-systemd) WITH_SYSTEMD=1 ;;
        --full) FULL_INSTALL=1 ;;
    esac
done

# ── Formatting ───────────────────────────────────────────────────────────────

bold=""
reset=""
green=""
red=""
yellow=""
if [ -t 1 ]; then
    bold="\033[1m"
    reset="\033[0m"
    green="\033[32m"
    red="\033[31m"
    yellow="\033[33m"
fi

info()  { printf "${bold}${green}  ▸${reset} %s\n" "$1"; }
warn()  { printf "${bold}${yellow}  ▸${reset} %s\n" "$1"; }
err()   { printf "${bold}${red}  ✗${reset} %s\n" "$1" >&2; }
die()   { err "$1"; exit 1; }

# ── Platform detection ───────────────────────────────────────────────────────

detect_platform() {
    OS="$(uname -s)"
    ARCH="$(uname -m)"

    case "$OS" in
        Linux)  OS_NAME="linux"  ;;
        Darwin) OS_NAME="macos"  ;;
        *)      die "Unsupported operating system: $OS" ;;
    esac

    case "$ARCH" in
        x86_64|amd64)       ARCH_NAME="x86_64"  ;;
        aarch64|arm64)      ARCH_NAME="aarch64"  ;;
        *)                  die "Unsupported architecture: $ARCH" ;;
    esac

    # Map to tarball naming convention from release.yml
    case "${OS_NAME}-${ARCH_NAME}" in
        linux-x86_64)   ARTIFACT="hebbs-linux-x86_64"   ;;
        linux-aarch64)  ARTIFACT="hebbs-linux-aarch64"   ;;
        macos-aarch64)  ARTIFACT="hebbs-macos-arm64"     ;;
        macos-x86_64)   ARTIFACT="hebbs-macos-x86_64" ;;
        *)              die "Unsupported platform: ${OS_NAME}-${ARCH_NAME}" ;;
    esac

    if [ "$FULL_INSTALL" = "1" ]; then
        ARTIFACT="${ARTIFACT}-full"
    fi

    info "Detected platform: ${OS} ${ARCH} → ${ARTIFACT}"
}

# ── Version resolution ───────────────────────────────────────────────────────

resolve_version() {
    if [ -n "${HEBBS_VERSION:-}" ]; then
        VERSION="$HEBBS_VERSION"
        info "Using requested version: ${VERSION}"
        return
    fi

    info "Resolving latest version..."

    RELEASE_JSON=""
    RELEASE_HTTP_CODE=""

    if command -v curl > /dev/null 2>&1; then
        RELEASE_HTTP_CODE=$(curl -sS -o /tmp/hebbs_release.json -w '%{http_code}' \
            "https://api.github.com/repos/${REPO}/releases/latest" 2>/dev/null) || true
        if [ -f /tmp/hebbs_release.json ]; then
            RELEASE_JSON=$(cat /tmp/hebbs_release.json)
            rm -f /tmp/hebbs_release.json
        fi
    elif command -v wget > /dev/null 2>&1; then
        RELEASE_JSON=$(wget -qO- "https://api.github.com/repos/${REPO}/releases/latest" 2>/dev/null) || true
    else
        die "Neither curl nor wget found. Install one of them and retry."
    fi

    if [ "$RELEASE_HTTP_CODE" = "404" ] || [ -z "$RELEASE_JSON" ]; then
        echo ""
        err "No releases found for ${REPO}."
        echo ""
        echo "  HEBBS has not published any binary releases yet."
        echo "  Check https://github.com/${REPO}/releases for updates,"
        echo "  or build from source: https://github.com/${REPO}#building-from-source"
        echo ""
        exit 1
    fi

    VERSION=$(printf '%s' "$RELEASE_JSON" \
        | grep '"tag_name"' | head -1 | sed 's/.*"tag_name": *"//;s/".*//')

    if [ -z "$VERSION" ]; then
        die "Could not parse version from release metadata. Set HEBBS_VERSION manually and retry."
    fi

    info "Latest version: ${VERSION}"
}

# ── Install directory ────────────────────────────────────────────────────────

resolve_install_dir() {
    if [ -n "${HEBBS_INSTALL_DIR:-}" ]; then
        INSTALL_DIR="$HEBBS_INSTALL_DIR"
    elif [ "$(id -u)" = "0" ]; then
        INSTALL_DIR="/usr/local/bin"
    else
        INSTALL_DIR="${HOME}/.hebbs/bin"
    fi

    mkdir -p "$INSTALL_DIR"
    info "Install directory: ${INSTALL_DIR}"
}

# ── Download ─────────────────────────────────────────────────────────────────

download() {
    URL="${BASE_URL}/download/${VERSION}/${ARTIFACT}.tar.gz"
    CHECKSUM_URL="${BASE_URL}/download/${VERSION}/checksums.txt"

    TMPDIR=$(mktemp -d)
    trap 'rm -rf "$TMPDIR"' EXIT

    TARBALL="${TMPDIR}/${ARTIFACT}.tar.gz"

    info "Downloading ${URL}..."

    if command -v curl > /dev/null 2>&1; then
        HTTP_CODE=$(curl -fSL -o "$TARBALL" -w '%{http_code}' "$URL" 2>/dev/null) || true
        if [ ! -f "$TARBALL" ] || [ "$HTTP_CODE" = "404" ]; then
            die "Download failed (HTTP ${HTTP_CODE}). Version ${VERSION} may not have a build for ${ARTIFACT}."
        fi
    elif command -v wget > /dev/null 2>&1; then
        wget -q -O "$TARBALL" "$URL" || die "Download failed. Version ${VERSION} may not have a build for ${ARTIFACT}."
    fi

    # Verify checksum
    if [ "${HEBBS_NO_VERIFY:-0}" != "1" ]; then
        CHECKSUM_FILE="${TMPDIR}/checksums.txt"
        if command -v curl > /dev/null 2>&1; then
            curl -fsSL -o "$CHECKSUM_FILE" "$CHECKSUM_URL" 2>/dev/null || true
        elif command -v wget > /dev/null 2>&1; then
            wget -q -O "$CHECKSUM_FILE" "$CHECKSUM_URL" 2>/dev/null || true
        fi

        if [ -f "$CHECKSUM_FILE" ] && [ -s "$CHECKSUM_FILE" ]; then
            EXPECTED=$(grep "${ARTIFACT}.tar.gz" "$CHECKSUM_FILE" | awk '{print $1}')
            if [ -n "$EXPECTED" ]; then
                if command -v sha256sum > /dev/null 2>&1; then
                    ACTUAL=$(sha256sum "$TARBALL" | awk '{print $1}')
                elif command -v shasum > /dev/null 2>&1; then
                    ACTUAL=$(shasum -a 256 "$TARBALL" | awk '{print $1}')
                else
                    warn "No sha256sum or shasum found — skipping checksum verification"
                    ACTUAL="$EXPECTED"
                fi

                if [ "$ACTUAL" != "$EXPECTED" ]; then
                    die "Checksum mismatch! Expected ${EXPECTED}, got ${ACTUAL}. The download may be corrupted."
                fi
                info "Checksum verified: OK"
            else
                warn "Checksum entry not found for ${ARTIFACT}.tar.gz — skipping verification"
            fi
        else
            warn "Checksums file not available — skipping verification"
        fi
    fi

    # Extract
    info "Extracting to ${INSTALL_DIR}..."
    tar xzf "$TARBALL" -C "$INSTALL_DIR"

    # Ensure binaries are executable
    for bin in hebbs hebbs-bench; do
        if [ -f "${INSTALL_DIR}/${bin}" ]; then
            chmod +x "${INSTALL_DIR}/${bin}"
        fi
    done
}

# ── Post-install ─────────────────────────────────────────────────────────────

post_install() {
    echo ""
    printf "${bold}${green}  ✓ HEBBS ${VERSION} installed successfully${reset}\n"
    echo ""

    for bin in hebbs hebbs-bench; do
        if [ -f "${INSTALL_DIR}/${bin}" ]; then
            printf "    ${green}✓${reset} %s\n" "${INSTALL_DIR}/${bin}"
        fi
    done

    echo ""

    # Check if install dir is in PATH
    case ":$PATH:" in
        *":${INSTALL_DIR}:"*) ;;
        *)
            warn "${INSTALL_DIR} is not in your PATH."
            echo ""
            echo "  Add it by running:"
            echo ""
            echo "    export PATH=\"${INSTALL_DIR}:\$PATH\""
            echo ""
            echo "  Or add that line to your ~/.bashrc, ~/.zshrc, or ~/.profile."
            echo ""
            ;;
    esac

    if [ "$FULL_INSTALL" = "1" ]; then
        echo "  Get started (local mode):"
        echo ""
        echo "    hebbs init .                            # initialize a vault"
        echo "    hebbs remember \"hello world\"             # store a memory"
        echo "    hebbs recall \"hello\"                     # recall it"
        echo "    hebbs panel                              # open the Memory Palace"
    else
        echo "  Get started:"
        echo ""
        echo "    hebbs login --endpoint http://server:8080 --api-key <key>"
        echo "    hebbs recall \"your query\"                # search memories"
        echo "    hebbs push ./docs                        # upload documents"
        echo ""
        echo "  Need the full engine for local mode?"
        echo "    curl -sSf https://hebbs.ai/install | sh -s -- --full"
    fi
    echo ""
}

# ── Systemd setup (Linux only) ────────────────────────────────────────────────

setup_systemd() {
    if [ "$OS_NAME" != "linux" ]; then
        die "--with-systemd is only supported on Linux"
    fi
    if [ "$(id -u)" != "0" ]; then
        die "--with-systemd requires root. Run with sudo."
    fi

    info "Setting up systemd service..."

    # Create system user if missing
    if ! id -u hebbs > /dev/null 2>&1; then
        useradd --system --no-create-home --shell /usr/sbin/nologin hebbs
        info "Created system user: hebbs"
    fi

    # Create directories
    mkdir -p /etc/hebbs
    mkdir -p /var/lib/hebbs
    chown hebbs:hebbs /var/lib/hebbs

    # Write default config if none exists
    if [ ! -f /etc/hebbs/hebbs.toml ]; then
        cat > /etc/hebbs/hebbs.toml <<'TOML'
[server]
grpc_port = 6380
http_port = 6381
bind_address = "0.0.0.0"
shutdown_timeout_secs = 15

[storage]
data_dir = "/var/lib/hebbs"

[embedding]
provider = "onnx"
auto_download = true

[auth]
enabled = true

[logging]
level = "info"
format = "json"
TOML
        info "Created default config: /etc/hebbs/hebbs.toml"
    else
        info "Config already exists: /etc/hebbs/hebbs.toml (unchanged)"
    fi

    # Write empty env file if none exists (for API keys)
    if [ ! -f /etc/hebbs/hebbs.env ]; then
        cat > /etc/hebbs/hebbs.env <<'ENV'
# HEBBS environment overrides and API keys.
# Uncomment and set values as needed.
#
# OPENAI_API_KEY=sk-...
# ANTHROPIC_API_KEY=sk-ant-...
# HEBBS_SERVER_SHUTDOWN_TIMEOUT_SECS=15
ENV
        chmod 600 /etc/hebbs/hebbs.env
        info "Created env file: /etc/hebbs/hebbs.env (mode 600)"
    fi

    # Install systemd unit
    UNIT_SRC="${INSTALL_DIR}/../share/hebbs/hebbs.service"
    UNIT_FALLBACK="$(cd "$(dirname "$0")" && pwd)/../systemd/hebbs.service"

    if [ -f "$UNIT_SRC" ]; then
        cp "$UNIT_SRC" /etc/systemd/system/hebbs.service
    elif [ -f "$UNIT_FALLBACK" ]; then
        cp "$UNIT_FALLBACK" /etc/systemd/system/hebbs.service
    else
        # Generate inline as fallback if no file is available
        cat > /etc/systemd/system/hebbs.service <<UNIT
[Unit]
Description=HEBBS Cognitive Memory Engine
Documentation=https://hebbs.ai/docs
After=network-online.target
Wants=network-online.target

[Service]
Type=simple
User=hebbs
Group=hebbs
ExecStart=${INSTALL_DIR}/hebbs serve --foreground
Restart=on-failure
RestartSec=5
TimeoutStopSec=20
StandardOutput=journal
StandardError=journal
SyslogIdentifier=hebbs
EnvironmentFile=-/etc/hebbs/hebbs.env
NoNewPrivileges=true
ProtectSystem=strict
ProtectHome=true
ReadWritePaths=/var/lib/hebbs
PrivateTmp=true
ProtectKernelTunables=true
ProtectKernelModules=true
ProtectControlGroups=true
RestrictSUIDSGID=true
RestrictNamespaces=true
LockPersonality=true
MemoryDenyWriteExecute=true
RestrictRealtime=true
LimitNOFILE=65536
LimitMEMLOCK=infinity

[Install]
WantedBy=multi-user.target
UNIT
    fi

    systemctl daemon-reload
    info "Installed systemd unit: hebbs.service"

    echo ""
    echo "  Enable and start HEBBS:"
    echo ""
    echo "    sudo systemctl enable --now hebbs"
    echo ""
    echo "  Check status:"
    echo ""
    echo "    sudo systemctl status hebbs"
    echo "    journalctl -u hebbs -f"
    echo ""
}

# ── Main ─────────────────────────────────────────────────────────────────────

main() {
    echo ""
    printf "${bold}  HEBBS Installer${reset}\n"
    echo ""

    detect_platform
    resolve_version
    resolve_install_dir
    download
    post_install

    if [ "$WITH_SYSTEMD" = "1" ]; then
        setup_systemd
    fi
}

main

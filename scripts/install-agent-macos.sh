#!/usr/bin/env bash
# NeuralMesh Provider Installer for macOS (Apple Silicon)
#
# Usage (one-liner):
#   curl -fsSL https://install.neuralmesh.io | bash
#
# Or run directly:
#   chmod +x install-agent-macos.sh && ./install-agent-macos.sh
#
# What this does:
#   1. Checks macOS version and Apple Silicon chip
#   2. Installs Homebrew (if missing)
#   3. Installs Python 3 and ML runtimes (MLX, PyTorch MPS, ONNX)
#   4. Downloads the neuralmesh-agent and nm CLI binaries
#   5. Creates the neuralmesh_worker isolation user
#   6. Installs the launchd daemon (auto-starts on boot)
#   7. Prints the provider invite link

set -euo pipefail

# ── Constants ─────────────────────────────────────────────────────────────────

NM_VERSION="${NM_VERSION:-latest}"
NM_INSTALL_DIR="/usr/local/bin"
NM_LOG_DIR="/var/log/neuralmesh"
NM_CONFIG_DIR="$HOME/.config/neuralmesh"
NM_TMP_DIR="/tmp/neuralmesh"
NM_GITHUB_ORG="neuralmesh-io"
NM_GITHUB_REPO="neuralmesh"
RELEASE_BASE="https://github.com/${NM_GITHUB_ORG}/${NM_GITHUB_REPO}/releases"

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
BOLD='\033[1m'
RESET='\033[0m'

# ── Helpers ────────────────────────────────────────────────────────────────────

info()    { echo -e "${CYAN}${BOLD}→${RESET} $*"; }
success() { echo -e "${GREEN}${BOLD}✓${RESET} $*"; }
warn()    { echo -e "${YELLOW}${BOLD}⚠${RESET} $*"; }
fail()    { echo -e "${RED}${BOLD}✗${RESET} $*"; exit 1; }

require_cmd() {
    command -v "$1" &>/dev/null || fail "Required command not found: $1"
}

# ── Banner ────────────────────────────────────────────────────────────────────

echo ""
echo -e "${CYAN}${BOLD}"
echo "  _   _                      _ __  __           _     "
echo " | \ | | ___ _   _ _ __ __ _| |  \/  | ___  ___| |__  "
echo " |  \| |/ _ \ | | | '__/ _\` | | |\/| |/ _ \/ __| '_ \ "
echo " | |\  |  __/ |_| | | | (_| | | |  | |  __/\__ \ | | |"
echo " |_| \_|\___|\__,_|_|  \__,_|_|_|  |_|\___||___/_| |_|"
echo ""
echo " Provider Installer — Apple Silicon Edition"
echo -e "${RESET}"

# ── System checks ─────────────────────────────────────────────────────────────

info "Checking system requirements..."

# macOS only
if [[ "$(uname -s)" != "Darwin" ]]; then
    fail "This installer is for macOS only. Linux support coming in Phase 4."
fi

# macOS 14+ (Sonoma) required
MACOS_VER="$(sw_vers -productVersion)"
MACOS_MAJOR="${MACOS_VER%%.*}"
if [[ "$MACOS_MAJOR" -lt 14 ]]; then
    fail "macOS 14 (Sonoma) or newer required. You have: $MACOS_VER"
fi
success "macOS $MACOS_VER"

# Apple Silicon required
ARCH="$(uname -m)"
if [[ "$ARCH" != "arm64" ]]; then
    fail "Apple Silicon (M1/M2/M3/M4) required. Detected: $ARCH"
fi

# Detect chip model
CHIP="$(sysctl -n machdep.cpu.brand_string 2>/dev/null || system_profiler SPHardwareDataType 2>/dev/null | grep 'Chip:' | awk '{print $2, $3}' || echo 'Apple Silicon')"
success "Chip: $CHIP"

# RAM
RAM_BYTES="$(sysctl -n hw.memsize)"
RAM_GB=$(( RAM_BYTES / 1073741824 ))
success "Unified memory: ${RAM_GB} GB"

if [[ "$RAM_GB" -lt 8 ]]; then
    warn "Minimum 8 GB recommended for useful jobs. You have ${RAM_GB} GB."
fi

echo ""

# ── Homebrew ──────────────────────────────────────────────────────────────────

info "Checking Homebrew..."
if ! command -v brew &>/dev/null; then
    info "Installing Homebrew..."
    /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
    # Add brew to PATH for arm64 Macs
    eval "$(/opt/homebrew/bin/brew shellenv)" 2>/dev/null || true
fi
success "Homebrew $(brew --version | head -1 | awk '{print $2}')"

# ── Python 3 ─────────────────────────────────────────────────────────────────

info "Checking Python 3..."
if ! command -v python3 &>/dev/null; then
    info "Installing Python 3..."
    brew install python@3.12
fi
PYTHON_VER="$(python3 --version 2>&1)"
success "$PYTHON_VER"

# Ensure pip is available
python3 -m ensurepip --upgrade &>/dev/null || true

# ── ML Runtimes ───────────────────────────────────────────────────────────────

echo ""
info "Installing ML runtimes (this may take a few minutes)..."

install_pip() {
    local pkg="$1"
    local name="${2:-$1}"
    info "  Installing $name..."
    if pip3 install --quiet --upgrade "$pkg" 2>/dev/null; then
        success "  $name"
    else
        warn "  $name install failed — skipping (can retry with: pip3 install $pkg)"
    fi
}

# MLX — primary runtime for Apple Silicon
install_pip "mlx>=0.16" "MLX"
install_pip "mlx-lm>=0.16" "MLX-LM"

# PyTorch MPS
install_pip "torch torchvision torchaudio" "PyTorch (MPS)"

# ONNX Runtime with CoreML EP
install_pip "onnxruntime" "ONNX Runtime (CoreML EP)"

# llama.cpp with Metal
info "  Installing llama-cpp-python (Metal)..."
if CMAKE_ARGS="-DGGML_METAL=on" FORCE_CMAKE=1 pip3 install --quiet llama-cpp-python 2>/dev/null; then
    success "  llama-cpp-python (Metal)"
else
    warn "  llama-cpp-python Metal install failed — skipping"
fi

# Common dependencies
install_pip "transformers huggingface-hub safetensors numpy" "ML utilities"

echo ""

# ── neuralmesh_worker user ────────────────────────────────────────────────────

info "Setting up isolation user (neuralmesh_worker)..."
if id "neuralmesh_worker" &>/dev/null; then
    success "neuralmesh_worker user already exists"
else
    # Find an unused UID in the 400–499 range
    WORKER_UID=450
    while dscl . -list /Users UniqueID | awk '{print $2}' | grep -q "^${WORKER_UID}$"; do
        WORKER_UID=$(( WORKER_UID + 1 ))
    done

    sudo dscl . -create /Users/neuralmesh_worker
    sudo dscl . -create /Users/neuralmesh_worker UserShell /usr/bin/false
    sudo dscl . -create /Users/neuralmesh_worker RealName "NeuralMesh Worker"
    sudo dscl . -create /Users/neuralmesh_worker UniqueID "$WORKER_UID"
    sudo dscl . -create /Users/neuralmesh_worker PrimaryGroupID 20
    sudo dscl . -create /Users/neuralmesh_worker NFSHomeDirectory /tmp/neuralmesh
    success "Created neuralmesh_worker (UID $WORKER_UID)"
fi

# ── Working directories ────────────────────────────────────────────────────────

info "Creating working directories..."
sudo mkdir -p "$NM_TMP_DIR"
sudo chown "neuralmesh_worker:staff" "$NM_TMP_DIR"
sudo chmod 750 "$NM_TMP_DIR"

sudo mkdir -p "$NM_LOG_DIR"
success "Directories ready"

# ── Download binaries ─────────────────────────────────────────────────────────

echo ""
info "Downloading NeuralMesh binaries..."

download_binary() {
    local name="$1"
    local dest="$2"

    if [[ "$NM_VERSION" == "latest" ]]; then
        local url="${RELEASE_BASE}/latest/download/${name}-darwin-arm64"
    else
        local url="${RELEASE_BASE}/download/${NM_VERSION}/${name}-darwin-arm64"
    fi

    info "  Downloading $name..."
    if curl -fsSL "$url" -o "/tmp/${name}" 2>/dev/null; then
        sudo install -m 755 "/tmp/${name}" "$dest"
        rm -f "/tmp/${name}"
        success "  $name → $dest"
    else
        warn "  Could not download $name from $url"
        warn "  Build from source: cargo build --release -p $name"
    fi
}

download_binary "neuralmesh-agent" "${NM_INSTALL_DIR}/neuralmesh-agent"
download_binary "nm" "${NM_INSTALL_DIR}/nm"

# ── Configuration ─────────────────────────────────────────────────────────────

info "Setting up configuration..."
mkdir -p "$NM_CONFIG_DIR"

AGENT_CONFIG="$NM_CONFIG_DIR/agent.toml"
if [[ ! -f "$AGENT_CONFIG" ]]; then
    cat > "$AGENT_CONFIG" <<EOF
# NeuralMesh Agent Configuration
# Edit then run: nm provider start

# Offer GPU when idle for this many minutes (screen locked, GPU < threshold)
idle_duration_minutes = 10

# GPU utilization % threshold to consider "idle"
idle_threshold_pct = 5.0

# Minimum price per hour you'll accept (NMC)
floor_price_nmc_per_hour = 0.05

# Maximum RAM to offer per job (GB). Default: 80% of total.
# max_job_ram_gb = 48

# Allowed job runtimes
allowed_runtimes = ["mlx", "torch-mps", "onnx-coreml", "llama-cpp"]
EOF
    success "Created $AGENT_CONFIG"
else
    success "Config already exists: $AGENT_CONFIG"
fi

# ── Account setup ──────────────────────────────────────────────────────────────

echo ""
info "Account setup..."
CLI_CONFIG="$NM_CONFIG_DIR/cli.toml"
if [[ ! -f "$CLI_CONFIG" ]]; then
    # Generate a random account ID (will be replaced with proper registration)
    ACCOUNT_ID="$(uuidgen | tr '[:upper:]' '[:lower:]')"
    cat > "$CLI_CONFIG" <<EOF
coordinator_url = "https://neuralmesh-coordinator-production-666f.up.railway.app"
ledger_url      = "https://neuralmesh-ledger-production-9e83.up.railway.app"
account_id      = "$ACCOUNT_ID"
EOF
    success "Account ID: $ACCOUNT_ID"
    info "  Save this ID — it's your identity on the network."
else
    ACCOUNT_ID="$(grep 'account_id' "$CLI_CONFIG" | awk -F'"' '{print $2}')"
    success "Existing account: $ACCOUNT_ID"
fi

# ── Install launchd daemon ─────────────────────────────────────────────────────

echo ""
info "Installing launchd daemon (auto-start on boot)..."

PLIST_PATH="/Library/LaunchDaemons/io.neuralmesh.agent.plist"
if [[ -f "$PLIST_PATH" ]]; then
    sudo launchctl unload -w "$PLIST_PATH" 2>/dev/null || true
fi

cat > "/tmp/io.neuralmesh.agent.plist" <<EOF
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN"
  "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>io.neuralmesh.agent</string>

    <key>ProgramArguments</key>
    <array>
        <string>${NM_INSTALL_DIR}/neuralmesh-agent</string>
        <string>--config</string>
        <string>${NM_CONFIG_DIR}/agent.toml</string>
    </array>

    <key>KeepAlive</key>
    <true/>

    <key>RunAtLoad</key>
    <true/>

    <key>StandardOutPath</key>
    <string>${NM_LOG_DIR}/agent.log</string>

    <key>StandardErrorPath</key>
    <string>${NM_LOG_DIR}/agent-error.log</string>

    <key>WorkingDirectory</key>
    <string>/tmp/neuralmesh</string>

    <key>EnvironmentVariables</key>
    <dict>
        <key>NM_ACCOUNT_ID</key>
        <string>${ACCOUNT_ID}</string>
        <key>RUST_LOG</key>
        <string>neuralmesh_agent=info</string>
    </dict>
</dict>
</plist>
EOF

sudo mv "/tmp/io.neuralmesh.agent.plist" "$PLIST_PATH"
sudo chown root:wheel "$PLIST_PATH"
sudo chmod 644 "$PLIST_PATH"

if [[ -f "${NM_INSTALL_DIR}/neuralmesh-agent" ]]; then
    sudo launchctl load -w "$PLIST_PATH"
    success "Agent daemon installed and started"
else
    warn "Agent binary not found — daemon installed but not started"
    warn "Build with: cargo build --release -p neuralmesh-agent"
    warn "Then: sudo launchctl load -w $PLIST_PATH"
fi

# ── Done ───────────────────────────────────────────────────────────────────────

echo ""
echo -e "${GREEN}${BOLD}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${RESET}"
echo -e "${GREEN}${BOLD}  NeuralMesh Provider Setup Complete!${RESET}"
echo -e "${GREEN}${BOLD}━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━${RESET}"
echo ""
echo -e "  ${BOLD}Your Mac:${RESET}        $CHIP (${RAM_GB}GB unified memory)"
echo -e "  ${BOLD}Account ID:${RESET}      $ACCOUNT_ID"
echo -e "  ${BOLD}Config:${RESET}          $AGENT_CONFIG"
echo -e "  ${BOLD}Logs:${RESET}            tail -f $NM_LOG_DIR/agent.log"
echo ""
echo -e "  ${CYAN}Next steps:${RESET}"
echo -e "    ${CYAN}nm provider status${RESET}          — check if agent is running"
echo -e "    ${CYAN}nm provider config --idle-minutes 5${RESET}  — reduce idle time"
echo -e "    ${CYAN}nm wallet balance${RESET}            — check your NMC balance"
echo -e "    ${CYAN}nm gpu benchmark${RESET}             — verify ML runtimes work"
echo ""
echo -e "  Your Mac will offer idle GPU time to the network when:"
echo -e "    • Screen is locked"
echo -e "    • GPU utilization < 5% for 10+ minutes"
echo ""
echo -e "  ${YELLOW}To stop:${RESET}  nm provider stop"
echo -e "  ${YELLOW}To uninstall:${RESET}  sudo launchctl unload $PLIST_PATH"
echo ""

#!/usr/bin/env bash
#
# Renders Formula/gitalong.rb from the in-repo template, fetching the SHA256
# sidecars over HTTPS from S3_PUBLIC_URL (so this can run from a laptop after
# CI has uploaded artifacts — no local build required), then opens a PR on
# the Homebrew tap identified by HOMEBREW_TAP_REPO_URL.
#
# Will switch to homebrew-core when that becomes possible by changing only
# HOMEBREW_TAP_REPO_URL.

set -euo pipefail

if [[ "${1-}" == "-h" || "${1-}" == "--help" ]]; then
    cat <<EOF
homebrew_pr.sh - Open a Homebrew tap PR with the new formula
Usage: ./scripts/homebrew_pr.sh <version>

Run this locally after CI has uploaded the release to R2. Fetches each
target's .sha256 sidecar over HTTPS, renders the formula template, and
pushes a branch + PR to the configured tap.

Required env (from .env):
  GITHUB_PERSONAL_ACCESS_TOKEN  GitHub PAT with \`repo\` scope on the tap
  HOMEBREW_TAP_REPO_URL         e.g. https://github.com/douglaslassance/homebrew-tap.git
  S3_PUBLIC_URL                 e.g. https://s3.douglaslassance.me
EOF
    exit 0
fi

if [[ -f .env ]]; then
    # shellcheck disable=SC1091
    source .env
fi

REPO_NAME="gitalong"
VERSION="${1:?version required}"

missing=()
[[ -z "${GITHUB_PERSONAL_ACCESS_TOKEN:-}" ]] && missing+=("GITHUB_PERSONAL_ACCESS_TOKEN")
[[ -z "${HOMEBREW_TAP_REPO_URL:-}" ]] && missing+=("HOMEBREW_TAP_REPO_URL")
[[ -z "${S3_PUBLIC_URL:-}" ]] && missing+=("S3_PUBLIC_URL")
if (( ${#missing[@]} )); then
    echo "Error: missing env vars: ${missing[*]}" >&2
    exit 1
fi

# Parse owner/repo from the tap URL for `gh pr create --repo`.
# Accepts both .git suffix and bare HTTPS; strips either.
TAP_SLUG=$(echo "$HOMEBREW_TAP_REPO_URL" \
    | sed -E 's#^https?://[^/]+/##; s#\.git$##')
if [[ ! "$TAP_SLUG" =~ ^[^/]+/[^/]+$ ]]; then
    echo "Error: HOMEBREW_TAP_REPO_URL=$HOMEBREW_TAP_REPO_URL did not parse as owner/repo." >&2
    exit 1
fi

# Targets that ship via Homebrew (macOS + Linux, both arches). Windows is
# uploaded to R2 but isn't a Homebrew target.
TARGETS=(
    "aarch64-apple-darwin"
    "x86_64-apple-darwin"
    "aarch64-unknown-linux-gnu"
    "x86_64-unknown-linux-gnu"
)

declare -A SHA

for target in "${TARGETS[@]}"; do
    archive="${REPO_NAME}-${VERSION}-${target}.tar.gz"
    sha_url="${S3_PUBLIC_URL%/}/${REPO_NAME}/${archive}.sha256"
    echo "Fetching SHA: ${sha_url}"
    if ! sha=$(curl -fsSL "$sha_url"); then
        echo "Error: could not fetch ${sha_url}. Has CD finished uploading version ${VERSION}?" >&2
        exit 1
    fi
    SHA["$target"]=$(echo "$sha" | awk '{print $1}')
done

# --- Render the formula from the template ---
TEMPLATE="Formula/${REPO_NAME}.rb.template"
if [[ ! -f "$TEMPLATE" ]]; then
    echo "Error: template ${TEMPLATE} not found." >&2
    exit 1
fi

RENDERED=$(mktemp)
sed \
    -e "s|{{VERSION}}|${VERSION}|g" \
    -e "s|{{SHA256_AARCH64_APPLE_DARWIN}}|${SHA["aarch64-apple-darwin"]}|g" \
    -e "s|{{SHA256_X86_64_APPLE_DARWIN}}|${SHA["x86_64-apple-darwin"]}|g" \
    -e "s|{{SHA256_AARCH64_UNKNOWN_LINUX_GNU}}|${SHA["aarch64-unknown-linux-gnu"]}|g" \
    -e "s|{{SHA256_X86_64_UNKNOWN_LINUX_GNU}}|${SHA["x86_64-unknown-linux-gnu"]}|g" \
    "$TEMPLATE" > "$RENDERED"

echo "--- Rendered Formula/${REPO_NAME}.rb ---"
cat "$RENDERED"
echo "----------------------------------------"

# --- Clone the tap and stage the formula ---
WORKTREE=$(mktemp -d)
trap 'rm -rf "$WORKTREE" "$RENDERED"' EXIT

# Inject the token via insteadOf so it doesn't end up in the repo's .git/config.
git -c "url.https://x-access-token:${GITHUB_PERSONAL_ACCESS_TOKEN}@github.com/.insteadOf=https://github.com/" \
    clone --depth=1 "$HOMEBREW_TAP_REPO_URL" "$WORKTREE"

cd "$WORKTREE"
git config user.email "actions@github.com"
git config user.name "github-actions[bot]"

BRANCH="${REPO_NAME}-${VERSION}"
git checkout -b "$BRANCH"

mkdir -p Formula
cp "$RENDERED" "Formula/${REPO_NAME}.rb"
git add "Formula/${REPO_NAME}.rb"

if git diff --cached --quiet; then
    echo "No changes against the tap — formula is already up to date."
    exit 0
fi

git commit -m "Update ${REPO_NAME} to ${VERSION}"
git -c "url.https://x-access-token:${GITHUB_PERSONAL_ACCESS_TOKEN}@github.com/.insteadOf=https://github.com/" \
    push --force-with-lease origin "$BRANCH"

# --- Open the PR ---
if ! command -v gh >/dev/null 2>&1; then
    echo "Installing gh CLI..."
    type apt-get >/dev/null 2>&1 && sudo apt-get install -y gh \
        || curl -fsSL https://cli.github.com/install.sh | sh
fi

GH_TOKEN="$GITHUB_PERSONAL_ACCESS_TOKEN" gh pr create \
    --repo "$TAP_SLUG" \
    --head "$BRANCH" \
    --base main \
    --title "Update ${REPO_NAME} to ${VERSION}" \
    --body "Automated bump from \`${REPO_NAME}\` ${VERSION} release. Built artifacts in R2; sha256s in this commit." \
    || echo "PR may already exist for ${BRANCH}; skipping create."

echo "Homebrew tap PR step complete."

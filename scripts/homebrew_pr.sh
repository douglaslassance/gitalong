#!/usr/bin/env bash
#
# Renders Formula/gitalong.rb from the in-repo template using the SHA256s
# in dist/ and the configured S3_PUBLIC_URL, then opens a PR on
# douglaslassance/homebrew-tap with the new formula.

set -euo pipefail

if [[ "${1-}" == "-h" || "${1-}" == "--help" ]]; then
    cat <<EOF
homebrew_pr.sh - Open a Homebrew tap PR with the new formula
Usage: ./scripts/homebrew_pr.sh <version>

Reads dist/gitalong-<version>-*.tar.gz.sha256 sidecars produced by the
build job and pushes a branch + PR to douglaslassance/homebrew-tap.

Required env:
  HOMEBREW_TAP_TOKEN  GitHub PAT with \`repo\` scope on the tap
  S3_PUBLIC_URL       Public download URL prefix, e.g. https://api.douglaslassance.me
EOF
    exit 0
fi

if [[ -f .env && -z "${CI:-}" ]]; then
    # shellcheck disable=SC1091
    source .env
fi

REPO_NAME="gitalong"
TAP_OWNER="douglaslassance"
TAP_REPO="homebrew-tap"
VERSION="${1:?version required}"

missing=()
[[ -z "${HOMEBREW_TAP_TOKEN:-}" ]] && missing+=("HOMEBREW_TAP_TOKEN")
[[ -z "${S3_PUBLIC_URL:-}" ]] && missing+=("S3_PUBLIC_URL")
if (( ${#missing[@]} )); then
    echo "Error: missing env vars: ${missing[*]}" >&2
    exit 1
fi

# Targets that ship via Homebrew (Linux + macOS, both arches).
TARGETS=(
    "aarch64-apple-darwin"
    "x86_64-apple-darwin"
    "aarch64-unknown-linux-gnu"
    "x86_64-unknown-linux-gnu"
)

declare -A SHA URL

for target in "${TARGETS[@]}"; do
    archive="${REPO_NAME}-${VERSION}-${target}.tar.gz"
    sha_file="dist/${archive}.sha256"
    if [[ ! -f "$sha_file" ]]; then
        echo "Error: missing ${sha_file} — was the build job's artifact uploaded?" >&2
        exit 1
    fi
    SHA["$target"]=$(awk '{print $1}' "$sha_file")
    URL["$target"]="${S3_PUBLIC_URL%/}/${REPO_NAME}/${archive}"
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
    -e "s|{{URL_AARCH64_APPLE_DARWIN}}|${URL["aarch64-apple-darwin"]}|g" \
    -e "s|{{SHA256_AARCH64_APPLE_DARWIN}}|${SHA["aarch64-apple-darwin"]}|g" \
    -e "s|{{URL_X86_64_APPLE_DARWIN}}|${URL["x86_64-apple-darwin"]}|g" \
    -e "s|{{SHA256_X86_64_APPLE_DARWIN}}|${SHA["x86_64-apple-darwin"]}|g" \
    -e "s|{{URL_AARCH64_UNKNOWN_LINUX_GNU}}|${URL["aarch64-unknown-linux-gnu"]}|g" \
    -e "s|{{SHA256_AARCH64_UNKNOWN_LINUX_GNU}}|${SHA["aarch64-unknown-linux-gnu"]}|g" \
    -e "s|{{URL_X86_64_UNKNOWN_LINUX_GNU}}|${URL["x86_64-unknown-linux-gnu"]}|g" \
    -e "s|{{SHA256_X86_64_UNKNOWN_LINUX_GNU}}|${SHA["x86_64-unknown-linux-gnu"]}|g" \
    "$TEMPLATE" > "$RENDERED"

echo "--- Rendered Formula/${REPO_NAME}.rb ---"
cat "$RENDERED"
echo "----------------------------------------"

# --- Clone the tap and stage the formula ---
WORKTREE=$(mktemp -d)
trap 'rm -rf "$WORKTREE" "$RENDERED"' EXIT

git -c "url.https://x-access-token:${HOMEBREW_TAP_TOKEN}@github.com/.insteadOf=https://github.com/" \
    clone --depth=1 "https://github.com/${TAP_OWNER}/${TAP_REPO}.git" "$WORKTREE"

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
git -c "url.https://x-access-token:${HOMEBREW_TAP_TOKEN}@github.com/.insteadOf=https://github.com/" \
    push --force-with-lease origin "$BRANCH"

# --- Open the PR ---
if ! command -v gh >/dev/null 2>&1; then
    echo "Installing gh CLI..."
    type apt-get >/dev/null 2>&1 && sudo apt-get install -y gh \
        || curl -fsSL https://cli.github.com/install.sh | sh
fi

# `gh` reads GH_TOKEN.
GH_TOKEN="$HOMEBREW_TAP_TOKEN" gh pr create \
    --repo "${TAP_OWNER}/${TAP_REPO}" \
    --head "$BRANCH" \
    --base main \
    --title "Update ${REPO_NAME} to ${VERSION}" \
    --body "Automated bump from \`${REPO_NAME}\` ${VERSION} release. Built artifacts in R2; sha256s in this commit." \
    || echo "PR may already exist for ${BRANCH}; skipping create."

echo "Homebrew tap PR step complete."

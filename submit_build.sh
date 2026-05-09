#!/usr/bin/env bash
#
# Renders Formula/gitalong.rb from the in-repo template, downloading each
# target's .tar.gz from S3_PUBLIC_URL and hashing it locally (so this can run
# from a laptop after CI has uploaded artifacts — no local build required),
# then pushes a release branch to the Homebrew tap identified by
# HOMEBREW_TAP_REPO_URL.
#
# Mirrors Peel's submit_build.sh: pass --pull-request to also open a PR
# against the tap. Without the flag the branch is just pushed and the tap
# owner merges manually.
#
# Will switch to homebrew-core when that becomes possible by changing only
# HOMEBREW_TAP_REPO_URL.

set -euo pipefail

PULL_REQUEST=false
ARGS=()
for arg in "$@"; do
    case "$arg" in
        -h|--help)
            cat <<EOF
submit_build.sh - Push (and optionally PR) the new formula to the Homebrew tap
Usage: ./submit_build.sh [--pull-request] <version>

Run this locally after CI has uploaded the release to R2. Downloads each
target's .tar.gz from S3_PUBLIC_URL, hashes it, renders the formula
template, and pushes a branch to the configured tap. With --pull-request,
also opens a PR back to the tap's main.

Required env (from .env):
  GITHUB_PERSONAL_ACCESS_TOKEN  GitHub PAT with \`repo\` scope on the tap
  HOMEBREW_TAP_REPO_URL         e.g. https://github.com/douglaslassance/homebrew-tap.git
  S3_PUBLIC_URL                 e.g. https://s3.douglaslassance.me
EOF
            exit 0
            ;;
        --pull-request) PULL_REQUEST=true ;;
        *) ARGS+=("$arg") ;;
    esac
done

if [[ -f .env ]]; then
    # shellcheck disable=SC1091
    source .env
fi

REPO_NAME="gitalong"
VERSION="${ARGS[0]:?version required}"

missing=()
[[ -z "${GITHUB_PERSONAL_ACCESS_TOKEN:-}" ]] && missing+=("GITHUB_PERSONAL_ACCESS_TOKEN")
[[ -z "${HOMEBREW_TAP_REPO_URL:-}" ]] && missing+=("HOMEBREW_TAP_REPO_URL")
[[ -z "${S3_PUBLIC_URL:-}" ]] && missing+=("S3_PUBLIC_URL")
if (( ${#missing[@]} )); then
    echo "Error: missing env vars: ${missing[*]}" >&2
    exit 1
fi

# Parse owner/repo from the tap URL for `gh pr create --repo`. Accepts both
# .git suffix and bare HTTPS; strips either.
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
    "aarch64-unknown-linux-gnu"
    "x86_64-unknown-linux-gnu"
)

declare -A SHA

SCRATCH=$(mktemp -d)
trap 'rm -rf "$SCRATCH"' EXIT

for target in "${TARGETS[@]}"; do
    archive="${REPO_NAME}-${VERSION}-${target}.tar.gz"
    archive_url="${S3_PUBLIC_URL%/}/${REPO_NAME}/${archive}"
    echo "Hashing ${archive_url}"
    if ! curl -fsSL -o "${SCRATCH}/${archive}" "$archive_url"; then
        echo "Error: could not fetch ${archive_url}. Has CD finished uploading version ${VERSION}?" >&2
        exit 1
    fi
    SHA["$target"]=$(shasum -a 256 "${SCRATCH}/${archive}" | awk '{print $1}')
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

BRANCH="bump-${REPO_NAME}-${VERSION}"
git checkout -b "$BRANCH"

mkdir -p Formula
cp "$RENDERED" "Formula/${REPO_NAME}.rb"
git add "Formula/${REPO_NAME}.rb"

if git diff --cached --quiet; then
    echo "No changes against the tap — formula is already up to date."
    exit 0
fi

git commit -m "${REPO_NAME} ${VERSION}"
git -c "url.https://x-access-token:${GITHUB_PERSONAL_ACCESS_TOKEN}@github.com/.insteadOf=https://github.com/" \
    push --force-with-lease origin "$BRANCH"

# --- Open the PR (only if asked) ---
if [ "$PULL_REQUEST" = true ]; then
    if ! command -v gh >/dev/null 2>&1; then
        echo "Installing gh CLI..."
        type apt-get >/dev/null 2>&1 && sudo apt-get install -y gh \
            || curl -fsSL https://cli.github.com/install.sh | sh
    fi

    # TODO: If the repo is not a fork make pull request to default branch, if it's a fork make pull request to upstream default branch.
    GH_TOKEN="$GITHUB_PERSONAL_ACCESS_TOKEN" gh pr create \
        --repo "$TAP_SLUG" \
        --head "$BRANCH" \
        --base main \
        --title "${REPO_NAME} ${VERSION}" \
        --body "Automated bump from \`${REPO_NAME}\` ${VERSION} release. Built artifacts in R2; sha256s in this commit." \
        || echo "PR may already exist for ${BRANCH}; skipping create."
fi

echo "Submission complete."

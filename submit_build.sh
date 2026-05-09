#!/usr/bin/env bash
#
# Renders Formula/gitalong.rb from the in-repo template, downloading each
# target's .tar.gz from S3_PUBLIC_URL and hashing it locally (so this can run
# from a laptop after CI has uploaded artifacts — no local build required),
# then pushes a release branch to the Homebrew tap identified by
# HOMEBREW_TAP_URL.
#
# Mirrors Peel's submit_build.sh: pass --pull-request to also open a PR
# against the tap. Without the flag the branch is just pushed and the tap
# owner merges manually.
#
# Will switch to homebrew-core when that becomes possible by changing only
# HOMEBREW_TAP_URL.

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
  HOMEBREW_TAP_URL         e.g. https://github.com/douglaslassance/homebrew-tap.git
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
[[ -z "${HOMEBREW_TAP_URL:-}" ]] && missing+=("HOMEBREW_TAP_URL")
[[ -z "${S3_PUBLIC_URL:-}" ]] && missing+=("S3_PUBLIC_URL")
if (( ${#missing[@]} )); then
    echo "Error: missing env vars: ${missing[*]}" >&2
    exit 1
fi

# Parse owner/repo from the tap URL for `gh pr create --repo`. Accepts both
# .git suffix and bare HTTPS; strips either.
TAP_SLUG=$(echo "$HOMEBREW_TAP_URL" \
    | sed -E 's#^https?://[^/]+/##; s#\.git$##')
if [[ ! "$TAP_SLUG" =~ ^[^/]+/[^/]+$ ]]; then
    echo "Error: HOMEBREW_TAP_URL=$HOMEBREW_TAP_URL did not parse as owner/repo." >&2
    exit 1
fi

# Targets that ship via Homebrew. brew is the macOS distribution channel
# only — Linux users `cargo install gitalong` and Windows users grab the
# zip from R2. CD still builds and uploads all five targets, but the
# formula only references the macOS ones.
TARGETS=(
    "aarch64-apple-darwin"
    "x86_64-apple-darwin"
)

# macOS ships bash 3.2 which has no associative arrays, so we build the
# per-target sed expressions inline as we walk the target list. Placeholder
# names follow the convention `{{SHA256_<TARGET_UPPERCASE_UNDERSCORED>}}`.

SCRATCH=$(mktemp -d)
trap 'rm -rf "$SCRATCH"' EXIT

SED_ARGS=("-e" "s|{{VERSION}}|${VERSION}|g")

for target in "${TARGETS[@]}"; do
    archive="${REPO_NAME}-${VERSION}-${target}.tar.gz"
    archive_url="${S3_PUBLIC_URL%/}/${REPO_NAME}/${archive}"
    echo "Hashing ${archive_url}"
    if ! curl -fsSL -o "${SCRATCH}/${archive}" "$archive_url"; then
        echo "Error: could not fetch ${archive_url}. Has CD finished uploading version ${VERSION}?" >&2
        exit 1
    fi
    sha=$(shasum -a 256 "${SCRATCH}/${archive}" | awk '{print $1}')
    placeholder=$(echo "$target" | tr 'a-z-' 'A-Z_')
    SED_ARGS+=("-e" "s|{{SHA256_${placeholder}}}|${sha}|g")
done

# --- Render the formula from the template ---
TEMPLATE="Formula/${REPO_NAME}.rb.template"
if [[ ! -f "$TEMPLATE" ]]; then
    echo "Error: template ${TEMPLATE} not found." >&2
    exit 1
fi

RENDERED=$(mktemp)
sed "${SED_ARGS[@]}" "$TEMPLATE" > "$RENDERED"

echo "--- Rendered Formula/${REPO_NAME}.rb ---"
cat "$RENDERED"
echo "----------------------------------------"

# --- Clone the tap and stage the formula ---
WORKTREE=$(mktemp -d)
trap 'rm -rf "$WORKTREE" "$RENDERED"' EXIT

# Inject the token via insteadOf so it doesn't end up in the repo's .git/config.
git -c "url.https://x-access-token:${GITHUB_PERSONAL_ACCESS_TOKEN}@github.com/.insteadOf=https://github.com/" \
    clone --depth=1 "$HOMEBREW_TAP_URL" "$WORKTREE"

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
    push --force origin "$BRANCH"

# --- Open the PR (only if asked) ---
if [ "$PULL_REQUEST" = true ]; then
    if ! command -v gh >/dev/null 2>&1; then
        echo "Installing gh CLI..."
        type apt-get >/dev/null 2>&1 && sudo apt-get install -y gh \
            || curl -fsSL https://cli.github.com/install.sh | sh
    fi

    # Detect whether the configured tap is a fork. If it is, the PR
    # targets the upstream's default branch (e.g. Homebrew/homebrew-cask)
    # with a head spec like `<owner>:<branch>`. Otherwise the PR is a
    # same-repo PR against the tap's own default branch.
    REPO_INFO=$(GH_TOKEN="$GITHUB_PERSONAL_ACCESS_TOKEN" \
        gh repo view "$TAP_SLUG" --json isFork,parent,defaultBranchRef 2>/dev/null)
    IS_FORK=$(echo "$REPO_INFO" | python3 -c \
        "import json,sys; print(json.load(sys.stdin).get('isFork', False))")

    if [ "$IS_FORK" = "True" ]; then
        PARENT_SLUG=$(echo "$REPO_INFO" | python3 -c \
            "import json,sys; print(json.load(sys.stdin)['parent']['nameWithOwner'])")
        PR_BASE=$(GH_TOKEN="$GITHUB_PERSONAL_ACCESS_TOKEN" \
            gh repo view "$PARENT_SLUG" --json defaultBranchRef -q '.defaultBranchRef.name')
        OWNER=$(echo "$TAP_SLUG" | cut -d/ -f1)
        PR_REPO="$PARENT_SLUG"
        PR_HEAD="${OWNER}:${BRANCH}"
        echo "Tap is a fork — PR will target ${PARENT_SLUG}:${PR_BASE}."
    else
        PR_BASE=$(echo "$REPO_INFO" | python3 -c \
            "import json,sys; print(json.load(sys.stdin)['defaultBranchRef']['name'])")
        PR_REPO="$TAP_SLUG"
        PR_HEAD="$BRANCH"
        echo "Tap is not a fork — PR will target ${TAP_SLUG}:${PR_BASE}."
    fi

    GH_TOKEN="$GITHUB_PERSONAL_ACCESS_TOKEN" gh pr create \
        --repo "$PR_REPO" \
        --head "$PR_HEAD" \
        --base "$PR_BASE" \
        --title "${REPO_NAME} ${VERSION}" \
        --body "Automated bump from \`${REPO_NAME}\` ${VERSION} release. Built artifacts in R2; sha256s in this commit." \
        || echo "PR may already exist for ${BRANCH}; skipping create."
fi

echo "Submission complete."

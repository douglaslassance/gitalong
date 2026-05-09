#!/usr/bin/env bash
#
# Uploads every gitalong-${VERSION}-*.{tar.gz,zip} artifact in dist/ to
# Cloudflare R2, sets public ACLs, updates the livecheck KV namespace, and
# purges the public CDN cache. Mirrors the Peel pattern but generalized for
# multi-target releases.

set -euo pipefail

if [[ "${1-}" == "-h" || "${1-}" == "--help" ]]; then
    cat <<EOF
upload_build.sh - Upload release artifacts to Cloudflare R2 and update KV
Usage: ./upload_build.sh <version>

Reads dist/gitalong-<version>-*.{tar.gz,zip}.
EOF
    exit 0
fi

# Pick up local .env when running outside CI (CI sets these via secrets).
if [[ -f .env && -z "${CI:-}" ]]; then
    # shellcheck disable=SC1091
    source .env
fi

REPO_NAME="gitalong"
VERSION="${1:-$(awk -F'"' '/^version =/ { print $2; exit }' Cargo.toml)}"

if [[ -z "${VERSION}" ]]; then
    echo "Error: version not provided and not found in Cargo.toml" >&2
    exit 1
fi

DIST_DIR="dist"

shopt -s nullglob
ARTIFACTS=("${DIST_DIR}"/${REPO_NAME}-${VERSION}-*.tar.gz "${DIST_DIR}"/${REPO_NAME}-${VERSION}-*.zip)
shopt -u nullglob

if (( ${#ARTIFACTS[@]} == 0 )); then
    echo "Error: no artifacts matching ${DIST_DIR}/${REPO_NAME}-${VERSION}-* found" >&2
    exit 1
fi

# --- Validate required environment ---
missing=()
[[ -z "${S3_ACCOUNT_ID:-}" ]] && missing+=("S3_ACCOUNT_ID")
[[ -z "${S3_ACCESS_KEY_ID:-}" ]] && missing+=("S3_ACCESS_KEY_ID")
[[ -z "${S3_SECRET_ACCESS_KEY:-}" ]] && missing+=("S3_SECRET_ACCESS_KEY")
[[ -z "${S3_BUCKET_NAME:-}" ]] && missing+=("S3_BUCKET_NAME")
if (( ${#missing[@]} )); then
    echo "Error: missing env vars: ${missing[*]}" >&2
    exit 1
fi

# --- AWS CLI setup (R2 speaks S3) ---
if ! command -v aws >/dev/null 2>&1; then
    echo "Installing AWS CLI..."
    if [[ "$(uname -s)" == "Darwin" ]]; then
        brew install awscli
    else
        curl -fsSL "https://awscli.amazonaws.com/awscli-exe-linux-$(uname -m).zip" -o /tmp/awscliv2.zip
        unzip -q /tmp/awscliv2.zip -d /tmp/awscli
        sudo /tmp/awscli/aws/install
    fi
fi

aws configure set aws_access_key_id "$S3_ACCESS_KEY_ID"
aws configure set aws_secret_access_key "$S3_SECRET_ACCESS_KEY"
aws configure set region auto

R2_ENDPOINT="https://${S3_ACCOUNT_ID}.r2.cloudflarestorage.com"

# --- Upload each artifact ---
for path in "${ARTIFACTS[@]}"; do
    file=$(basename "$path")
    key="${REPO_NAME}/${file}"
    echo "Uploading ${file} -> s3://${S3_BUCKET_NAME}/${key}"
    aws s3 cp "$path" "s3://${S3_BUCKET_NAME}/${key}" --endpoint-url "$R2_ENDPOINT"

    # Best-effort public ACL — silently noop on buckets where ACLs are off.
    aws s3api put-object-acl \
        --bucket "$S3_BUCKET_NAME" \
        --key "$key" \
        --acl public-read \
        --endpoint-url "$R2_ENDPOINT" >/dev/null 2>&1 || true
done

# --- KV update (Cloudflare Worker reads this for livecheck) ---
if [[ -z "${CLOUDFLARE_API_TOKEN:-}" || -z "${CLOUDFLARE_KV_NAMESPACE_ID:-}" ]]; then
    echo "Skipping KV update (CLOUDFLARE_API_TOKEN or CLOUDFLARE_KV_NAMESPACE_ID not set)."
elif [[ "$VERSION" =~ (alpha|beta|rc|pre|dev) ]]; then
    echo "Skipping KV update (pre-release version: $VERSION)."
else
    KV_URL="https://api.cloudflare.com/client/v4/accounts/${S3_ACCOUNT_ID}/storage/kv/namespaces/${CLOUDFLARE_KV_NAMESPACE_ID}/values/${REPO_NAME}"
    CURRENT_KV=$(curl -s -H "Authorization: Bearer $CLOUDFLARE_API_TOKEN" "$KV_URL" || echo '')
    CURRENT_LATEST=$(echo "$CURRENT_KV" | grep -o '"latest":"[^"]*"' | cut -d'"' -f4 || true)

    HIGHER=$(printf '%s\n' "$CURRENT_LATEST" "$VERSION" | sort -V | tail -1)
    if [[ -z "$CURRENT_LATEST" || ( "$HIGHER" == "$VERSION" && "$CURRENT_LATEST" != "$VERSION" ) ]]; then
        EXISTING_DOWNLOADS=$(echo "$CURRENT_KV" | python3 -c '
import sys, json
try:
    d = json.load(sys.stdin)
    print(json.dumps(d.get("downloads", {})))
except Exception:
    print("{}")
')
        # gitalong ships .tar.gz everywhere except Windows MSVC, where it
        # ships .zip. The Worker reads these to resolve `/{app}/download/...`
        # URLs to actual R2 keys.
        NEW_VALUE=$(python3 -c '
import json, sys
data = {
    "latest": sys.argv[1],
    "downloads": json.loads(sys.argv[2]),
    "extension": ".tar.gz",
    "extension_overrides": {"x86_64-pc-windows-msvc": ".zip"},
}
print(json.dumps(data))
' "$VERSION" "$EXISTING_DOWNLOADS")

        echo "Updating KV: ${CURRENT_LATEST:-none} -> ${VERSION}"
        KV_RESULT=$(curl -s -X PUT "$KV_URL" \
            -H "Authorization: Bearer $CLOUDFLARE_API_TOKEN" \
            -H "Content-Type: application/json" \
            --data "$NEW_VALUE")
        if echo "$KV_RESULT" | grep -q '"success":true'; then
            echo "KV updated."
        else
            echo "KV update failed: $KV_RESULT" >&2
            exit 1
        fi
    else
        echo "Skipping KV update (${VERSION} is not newer than ${CURRENT_LATEST})."
    fi
fi

# --- Purge Cloudflare cache for the new artifacts ---
if [[ -n "${CLOUDFLARE_API_TOKEN:-}" && -n "${CLOUDFLARE_ZONE_ID:-}" && -n "${S3_PUBLIC_URL:-}" ]]; then
    files_json="["
    sep=""
    for path in "${ARTIFACTS[@]}"; do
        file=$(basename "$path")
        files_json+="${sep}\"${S3_PUBLIC_URL%/}/${REPO_NAME}/${file}\""
        sep=","
    done
    files_json+="]"

    echo "Purging Cloudflare cache for ${#ARTIFACTS[@]} URLs..."
    PURGE=$(curl -s -X POST "https://api.cloudflare.com/client/v4/zones/${CLOUDFLARE_ZONE_ID}/purge_cache" \
        -H "Authorization: Bearer ${CLOUDFLARE_API_TOKEN}" \
        -H "Content-Type: application/json" \
        --data "{\"files\":${files_json}}")
    if echo "$PURGE" | grep -q '"success":true'; then
        echo "Cache purged."
    else
        echo "Cache purge failed (continuing): $PURGE" >&2
    fi
fi

echo "Upload complete: ${#ARTIFACTS[@]} artifacts for ${REPO_NAME} ${VERSION}."

#!/usr/bin/env bash
# SPDX-FileCopyrightText: NONE
# SPDX-License-Identifier: CC0-1.0
set -euo pipefail

if [[ $# != 3 ]]; then
    echo "Usage: $0 SOURCE_REPO SITE_DIRECTORY PAGES_URL" >&2
    exit 1
fi
source_repo=$(realpath "$1")
site=$2
pages_url=${3%/}
: "${FLATPAK_GPG_FINGERPRINT:?A persistent signing key is required}"
root=$(git rev-parse --show-toplevel)
app_id=io.github.yamada_sexta.FractalSexta
app_ref=app/$app_id/x86_64/nightly
locale_ref=runtime/$app_id.Locale/x86_64/nightly
public_key=${FLATPAK_GPG_PUBLIC_KEY:-$root/data/fractal-sexta-signing-key.asc}

# Refuse to replace an unrelated directory or publish an incomplete build.
[[ ! -e "$site" ]]
ostree --repo="$source_repo" rev-parse "$app_ref" >/dev/null
fingerprint=$(gpg --show-keys --with-colons "$public_key" | awk -F: '$1 == "fpr" {print $10; exit}')
[[ "$fingerprint" == "$FLATPAK_GPG_FINGERPRINT" ]]
mkdir -p "$site/repo"
repo=$(realpath "$site/repo")
ostree --repo="$repo" init --mode=archive-z2

# Only a missing summary is a first deployment; network/signature errors must fail.
status=$(curl --silent --show-error --location --output "$site/previous-summary" \
    --write-out '%{http_code}' "$pages_url/repo/summary")
case "$status" in
    200)
        ostree --repo="$repo" remote add --gpg-import="$public_key" \
            --set=gpg-verify-summary=true published "$pages_url/repo/"
        previous_refs=$(ostree --repo="$repo" remote refs published)
        for ref in "$app_ref" "$locale_ref"; do
            if grep -Fxq "published:$ref" <<< "$previous_refs"; then
                ostree --repo="$repo" pull --mirror --depth=0 published "$ref"
            fi
        done
        ostree --repo="$repo" rev-parse "$app_ref" >/dev/null
        ostree --repo="$repo" remote delete published
        ;;
    404) ;;
    *) echo "Cannot restore the published repository: HTTP $status" >&2; exit 1 ;;
esac
rm "$site/previous-summary"

for ref in "$app_ref" "$locale_ref"; do
    if ostree --repo="$source_repo" rev-parse "$ref" >/dev/null 2>&1; then
        flatpak build-commit-from --src-repo="$source_repo" --src-ref="$ref" \
            --force --timestamp=NOW --no-update-summary \
            --gpg-sign="$FLATPAK_GPG_FINGERPRINT" "$repo" "$ref"
    elif ostree --repo="$repo" rev-parse "$ref" >/dev/null 2>&1; then
        ostree --repo="$repo" refs --delete "$ref"
    fi
done
flatpak build-update-repo --title="Fractal Sexta" --default-branch=nightly \
    --gpg-import="$public_key" --gpg-sign="$FLATPAK_GPG_FINGERPRINT" \
    --generate-static-deltas --prune --prune-depth=1 "$repo"
ostree --repo="$repo" fsck

gpg_key=$(gpg --export "$FLATPAK_GPG_FINGERPRINT" | base64 --wrap=0)
[[ -n "$gpg_key" ]]
cat > "$site/fractal-sexta.flatpakrepo" <<EOF
[Flatpak Repo]
Title=Fractal Sexta
Url=$pages_url/repo/
Homepage=$pages_url/
Comment=Personal Fractal Sexta builds
Description=Rolling x86_64 builds of Fractal Sexta
Icon=$pages_url/icon.svg
DefaultBranch=nightly
GPGKey=$gpg_key
EOF
cat > "$site/fractal-sexta.flatpakref" <<EOF
[Flatpak Ref]
Title=Fractal Sexta
Name=$app_id
Branch=nightly
Url=$pages_url/repo/
SuggestRemoteName=fractal-sexta
Homepage=$pages_url/
Icon=$pages_url/icon.svg
RuntimeRepo=https://nightly.gnome.org/gnome-nightly.flatpakrepo
IsRuntime=false
GPGKey=$gpg_key
EOF
cp "$root/.github/pages/index.html" "$site/index.html"
cp "$root/data/icons/org.gnome.Fractal.svg" "$site/icon.svg"
cp "$public_key" "$site/signing-key.asc"
printf '%s\n' "$FLATPAK_GPG_FINGERPRINT" > "$site/signing-key.txt"
touch "$site/.nojekyll"
size=$(du --bytes --summarize "$site" | cut -f1)
if (( size > 900 * 1024 * 1024 )); then
    echo "Pages artifact exceeds the 900 MiB budget: $size bytes" >&2
    exit 1
fi
echo "Signed Pages repository ready: $size bytes"

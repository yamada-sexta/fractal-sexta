#!/usr/bin/env bash
# SPDX-FileCopyrightText: NONE
# SPDX-License-Identifier: CC0-1.0
set -euo pipefail

root=$(git rev-parse --show-toplevel)
work=$(mktemp -d)
server_pid=
cleanup() {
    if [[ -n "$server_pid" ]]; then kill "$server_pid" 2>/dev/null || true; fi
    gpgconf --kill gpg-agent
    rm -rf "$work"
}
trap cleanup EXIT
export GNUPGHOME=$work/keys
mkdir -m 700 "$GNUPGHOME"
gpg --batch --pinentry-mode loopback --passphrase '' --quick-generate-key \
    'Fractal Sexta CI test' ed25519 sign 0
export FLATPAK_GPG_FINGERPRINT
FLATPAK_GPG_FINGERPRINT=$(gpg --list-secret-keys --with-colons | awk -F: '$1 == "fpr" {print $10; exit}')
export FLATPAK_GPG_PUBLIC_KEY=$work/public-key.asc
gpg --armor --export "$FLATPAK_GPG_FINGERPRINT" > "$FLATPAK_GPG_PUBLIC_KEY"

mkdir "$work/http"
python3 - "$work/http" "$work/port" <<'PY' &
import functools
import http.server
import pathlib
import sys

handler = functools.partial(http.server.SimpleHTTPRequestHandler, directory=sys.argv[1])
server = http.server.ThreadingHTTPServer(('127.0.0.1', 0), handler)
pathlib.Path(sys.argv[2]).write_text(str(server.server_port))
server.serve_forever()
PY
server_pid=$!
for _ in {1..50}; do
    [[ -s "$work/port" ]] && break
    sleep 0.1
done
pages_url=http://127.0.0.1:$(cat "$work/port")/site
app_id=io.github.yamada_sexta.FractalSexta
app_ref=app/$app_id/x86_64/nightly
mkdir -p "$work/app/files/bin"
cat > "$work/app/metadata" <<EOF
[Application]
name=$app_id
runtime=org.gnome.Platform/x86_64/master
sdk=org.gnome.Sdk/x86_64/master
command=fractal
EOF
printf '#!/bin/sh\necho first-build\n' > "$work/app/files/bin/fractal"
chmod +x "$work/app/files/bin/fractal"
flatpak build-finish "$work/app"
flatpak build-export "$work/source" "$work/app" nightly
ostree --repo="$work/source" refs --create="runtime/$app_id.Debug/x86_64/nightly" "$app_ref"

# First publication, with no previously hosted repository.
bash "$root/.github/scripts/publish-flatpak.sh" "$work/source" "$work/site1" "$pages_url"
ln -s "$work/site1" "$work/http/site"
first=$(ostree --repo="$work/site1/repo" rev-parse "$app_ref")
grep -Fxq "GPGKey=$(gpg --export "$FLATPAK_GPG_FINGERPRINT" | base64 --wrap=0)" \
    "$work/site1/fractal-sexta.flatpakref"
export FLATPAK_USER_DIR=$work/installation
flatpak remote-add --user test "$work/site1/fractal-sexta.flatpakrepo"
flatpak install --user --noninteractive --no-deps test "$app_id//nightly"
[[ $(flatpak info --user --show-commit "$app_id") == "$first" ]]

# A second publication retains the previous signed commit and updates clients.
printf '#!/bin/sh\necho second-build\n' > "$work/app/files/bin/fractal"
flatpak build-export "$work/source" "$work/app" nightly
bash "$root/.github/scripts/publish-flatpak.sh" "$work/source" "$work/site2" "$pages_url"
second=$(ostree --repo="$work/site2/repo" rev-parse "$app_ref")
[[ "$first" != "$second" ]]
ostree --repo="$work/site2/repo" show "$first" >/dev/null
ln -s "$work/site2" "$work/http/site-next"
mv -Tf "$work/http/site-next" "$work/http/site"
flatpak update --user --noninteractive --no-deps "$app_id"
[[ $(flatpak info --user --show-commit "$app_id") == "$second" ]]
[[ -s "$work/site2/repo/summary.sig" ]]
if ostree --repo="$work/site2/repo" refs | grep -q '\.Debug/'; then
    echo "Debug refs were published" >&2
    exit 1
fi

# Trust failures and incomplete builds must stop before a deployable site exists.
printf 'invalid signature\n' > "$work/site2/repo/summary.sig"
if bash "$root/.github/scripts/publish-flatpak.sh" "$work/source" "$work/bad-signature" "$pages_url"; then
    echo "Invalid repository signature was accepted" >&2
    exit 1
fi
if FLATPAK_GPG_FINGERPRINT='' bash "$root/.github/scripts/publish-flatpak.sh" \
    "$work/source" "$work/missing-key" "$pages_url"; then
    echo "Missing signing credentials were accepted" >&2
    exit 1
fi
ostree --repo="$work/empty" init --mode=archive-z2
if bash "$root/.github/scripts/publish-flatpak.sh" "$work/empty" "$work/empty-site" "$pages_url"; then
    echo "An incomplete build was accepted" >&2
    exit 1
fi
echo "Signed installation, update, retention, and failure checks passed"

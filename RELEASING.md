# Publishing Fractal Sexta

## Rolling builds

Every passing push to `main` publishes an x86_64 Flatpak on the `nightly` branch. Pull requests build
without publishing. A manual run of the CI workflow on `main` can retry a failed publication.
The build version includes the workflow run number and source commit.

GitHub Pages is configured to deploy from Actions. The publication job waits for the Flatpak build,
serializes deployments, and skips superseded commits. The website contains installation descriptors
and a signed OSTree repository.

## Signing configuration

The repository uses these GitHub Actions settings:

* Secret `FLATPAK_GPG_PRIVATE_KEY`: the ASCII-armored private signing key, without a passphrase.
* Variable `FLATPAK_GPG_FINGERPRINT`: the full public fingerprint.
* Tracked public key: `data/fractal-sexta-signing-key.asc`.

Keep the same key across builds so existing installations can authenticate updates. Never commit
private key material. The initial private key backup is in
`~/.local/share/fractal-sexta/signing/`, with access restricted to its owner.

A key rotation requires updating the public key and both Actions settings, with an explicit trust
transition for installed clients. Re-running CI must not generate a new key.

## Build caches

Flatpak builds cache Cargo home and target directories outside the source checkout, plus Flatpak's
module cache and source downloads. The installed SDK and runtime commit hashes invalidate Flatpak
caches when the nightly build environment changes.

The `cargo-cache-dir` Meson option selects persistent Cargo storage for CI. Local builds continue
using their own build directories by default. Signing material is never cached.

## Repository retention and failures

Publication restores the current signed app and locale refs from Pages, imports the new build,
and regenerates signed AppStream metadata, summary, and static deltas. The current and previous
application generations are retained. Debug refs and runtimes are not published.

The site is capped at 900 MiB, below GitHub Pages' 1 GB limit. A build, signature, restore, validation,
or size failure stops before deployment and leaves the previous site available. First deployment
accepts a missing repository summary (HTTP 404); other restore failures are fatal.

After publication, verify installation from the `.flatpakref` and updates with `flatpak update`.
GitLab automation is retained independently; this GitHub repository does not publish to Flathub.

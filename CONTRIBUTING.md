# Contributing to Fractal Sexta

FractalSexta is a fork of Fractal. Its AI policy is not Fractal's.

## Getting Started

* [Learn Rust](https://www.rust-lang.org/learn)
* [GUI development with Rust and GTK 4](https://gtk-rs.org/gtk4-rs/stable/latest/book)
* [gtk-rs website](https://gtk-rs.org/)
* [GNOME Development Center](https://developer.gnome.org/)

Issues and pull requests: <https://github.com/yamada-sexta/fractal-sexta>

## Build Instructions

### Prerequisites

FractalSexta is written in Rust. You need Rust (minimum in `Cargo.toml` as `workspace.package.rust-version`)
and Cargo. The [pre-commit hook](#pre-commit) needs nightly:

```sh
rustup toolchain install nightly
```

Flatpak builds (GNOME Builder or the command line) need the remotes and the Rust freedesktop.org
extension:

```sh
flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
flatpak remote-add --user --if-not-exists gnome-nightly https://nightly.gnome.org/gnome-nightly.flatpakrepo

flatpak install --user gnome-nightly org.gnome.Sdk//master org.gnome.Platform//master

flatpak install --user flathub org.freedesktop.Sdk.Extension.rust-stable
```

A manual flatpak build also needs `flatpak-builder`, or the `org.flatpak.Builder` flatpak from
Flathub.

### GNOME Builder

[GNOME Builder](https://apps.gnome.org/Builder/) with [Flatpak](https://flatpak.org/) is the
recommended way to build and install FractalSexta. See the [Builder
docs](https://builder.readthedocs.io/) for cloning and building.

A build terminal (header bar “+” → “New build terminal”, or
<kbd>Shift</kbd>+<kbd>Ctrl</kbd>+<kbd>Alt</kbd>+<kbd>T</kbd>) opens in `_build`.

### Foundry

[Foundry](https://gitlab.gnome.org/GNOME/foundry) can develop in a Flatpak environment (`foundry`
package):

```sh
foundry init
foundry run
```

Foundry uses `.foundry/cache/build`. Re-run `foundry run` after changes. Clippy and similar
commands:

```sh
foundry devenv -- {COMMAND}
```

### fenv

[fenv](https://gitlab.gnome.org/ZanderBrown/fenv) is Flatpak-only:

```sh
cargo install --git https://gitlab.gnome.org/ZanderBrown/fenv fenv
fenv gen build-aux/io.github.yamada_sexta.FractalSexta.Devel.json
fenv build
fenv run
```

fenv uses `_build`. Re-run the last two commands after changes. Other commands:

```sh
fenv exec -- {COMMAND}
```

### Install the flatpak

Notifications, command-line arguments, and similar features need an installed app.

GNOME Builder can export a flatpak after a successful build:

```sh
flatpak install --user --bundle path/to/io.github.yamada_sexta.FractalSexta.Devel.flatpak
```

Or with flatpak-builder (`flatpak run org.flatpak.Builder` works the same):

```sh
flatpak-builder --user --install app build-aux/io.github.yamada_sexta.FractalSexta.Devel.json
```

Remove it with:

```sh
flatpak remove --delete-data io.github.yamada_sexta.FractalSexta.Devel
```

### GNU/Linux

Host builds need Meson and Ninja. Flatpak is still the recommended path.

```sh
meson setup --prefix=/usr/local _build
ninja -C _build
sudo ninja -C _build install
```

### Workspace

Rust sources live under `crates/`. The `fractal` executable is in `fractal-app`;
its templates remain there so installed GResource paths stay stable. The other
members separate configuration, shared utilities, GTK models, media, Matrix
backend, widgets, verification, accounts, room details, room history, and the
session view. All members are private and share dependency versions and lints.

Configure Meson first to generate application configuration. For direct Cargo
checks in that build environment:

```sh
export FRACTAL_BUILD_DIR="$PWD/_build"
export FRACTAL_VERSION=14.1.0
cargo check --workspace --all-targets --locked
cargo check -p fractal-room-history-ui --locked
```

A UI-only edit now invalidates that crate and its dependents instead of the entire
application. Changes to shared backend interfaces still rebuild dependent UI
crates. Full clean builds still compile the same external dependencies. Use
`cargo build -p fractal --timings` in the build environment to inspect actual
compilation costs; the split does not guarantee faster clean builds.

The Matrix SDK already stores state, encryption keys, events, and downloaded
media in SQLite. Decoded static thumbnails use a separate 32 MiB per-session
weighted cache; logout clears it. Animated decoders are not retained in that
cache. This is a pixel budget, not a total process or GPU memory limit.

## Pre-commit

Contributions must be formatted. The build installs a pre-commit hook that runs the `hooks/checks`
crate (`rustfmt` and other checks). Run it before opening a pull request.

Also run [Clippy](https://doc.rust-lang.org/stable/clippy/index.html) in a build environment so it
shares the FractalSexta build cache:

```sh
meson compile -C {BUILD_DIRECTORY} crates/fractal-app/src/cargo-clippy
```

Omit `-C {BUILD_DIRECTORY}` when already in the build directory.

## Commit

Follow the [GNOME commit message
guidelines](https://handbook.gnome.org/development/commit-messages.html). The summary line starts
with a tag for the area of the app that changed.

## Pull Request

Pass the prerequisites of the [Change Submission
Guide](https://handbook.gnome.org/development/change-submission.html).

The fork must be public or CI cannot run.

One commit: use its title as the pull request title. Several commits: a title that summarizes them.
Split unrelated tasks into separate pull requests.

Describe what the change does. UI or UX changes need screenshots in light and dark mode, and/or a
screencast.

Mention the related issue if there is one. GitHub closes it on merge with `Closes #XXXX` or `Fixes
#XXXX`.

Keep history clean. Fixes go in with
[amend](https://git-scm.com/docs/git-commit#Documentation/git-commit.txt---amend) or
[fixup](https://git-scm.com/docs/git-commit#Documentation/git-commit.txt---fixupamendrewordltcommitgt)
and [squash](https://git-scm.com/docs/git-rebase#Documentation/git-rebase.txt---autosquash), then
[force push](https://git-scm.com/docs/git-push#Documentation/git-push.txt--f).

## AI contributions

AI contributions are welcome. A human must review the code. Fully automated responses are not
accepted.

Name the model that contributed, for example GPT6 Astra or Grok 4.7. That mark is required.

Translating issue and comment text to English may use non-generative machine translation such as
DeepL.

## GitHub Actions

Open pull requests and fork bug reports on [GitHub](https://github.com/yamada-sexta/fractal-sexta).
GitHub Actions builds the Flatpak, verifies the exported repository, and publishes passing main
builds. The workflow also runs workspace tests with a virtual display before exporting the app.
Run source linting locally before pushing.

Run the repository checks locally with:

```sh
cargo run --locked --manifest-path hooks/checks/Cargo.toml -- --verbose --force-install --cargo-binstall
bash .github/scripts/test-publish-flatpak.sh
```

The second command requires Flatpak, OSTree, GnuPG, curl, and Python. It generates a temporary
signing key and tests signed installation, update, retention, and rejection of corrupt repositories.
It uses an isolated Flatpak installation and does not change your installed applications.

GitLab CI and issue templates remain available. Its manifest and application ID follow the fork. The
fork retains upstream translation catalogs and internal resource names to make merging easier. For
upstream development guidance, see [Fractal's
documentation](https://gitlab.gnome.org/World/fractal/-/blob/main/CONTRIBUTING.md).

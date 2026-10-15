[![GitHub CI](https://github.com/yamada-sexta/fractal-sexta/actions/workflows/ci.yml/badge.svg)](https://github.com/yamada-sexta/fractal-sexta/actions/workflows/ci.yml)
[![Upstream Fractal](https://img.shields.io/badge/upstream-Fractal-blue)](https://gitlab.gnome.org/World/fractal/)

<div align="center">

<img
    src="data/icons/org.gnome.Fractal.svg"
    alt=""
/>

# Fractal Sexta

</div>

Fractal Sexta is a personal fork of [Fractal](https://gitlab.gnome.org/World/fractal/), the GNOME
Matrix client written in Rust. This project exist due to the extreme Anti-AI policies and the human
slop codebase.

Why this fork?

- sensible VRAM usage

<img width="919" height="101" alt="image" src="https://github.com/user-attachments/assets/aa14002b-a857-45e9-a722-cc0db33a85f4" />

- background notification/system tray

<div align="center">
<img
    src="screenshots/main.png"
    alt="Fractal’s main window"
    width="882"
    height="672"
/>
</div>

Highlights:

* Find rooms to discuss your favorite topics, or talk privately to people, securely thanks to
  end-to-end encryption
* Send rich formatted messages, files, or your current location
* Reply to specific messages, react with emoji, edit or remove messages
* View images, and play audio and video directly in the conversation
* See who has read messages, and who is typing
* Log into multiple accounts at once (with Single-Sign On support)

## Contents

<!-- toc -->
* [Installation instructions](#installation-instructions)
* [Security Best Practices](#security-best-practices)
* [Contributing](#contributing)
* [Frequently Asked Questions](#frequently-asked-questions)
* [The origin of Fractal](#the-origin-of-fractal)
* [Code of Conduct](#code-of-conduct)
<!-- /toc -->

## Installation instructions

Flatpak is the recommended installation method. For installing any of our Flatpaks, you need to
make sure your system is [set up with the Flathub remote](https://flathub.org/setup).

All of our Flatpaks can be installed in parallel, offering you the opportunity to try out the
development version while keeping the stable release around for daily use.

### Fractal Sexta rolling builds

Install the fork from its signed GitHub Pages repository:

```sh
flatpak install --user https://yamada-sexta.github.io/fractal-sexta/fractal-sexta.flatpakref
flatpak run io.github.yamada_sexta.FractalSexta//nightly
```

The install descriptor also configures the GNOME nightly runtime source. Builds are x86_64 only
and publish after all GitHub Actions checks pass on `main`. This is a personal, publicly downloadable
Flatpak repository; it does not require authentication.

For subsequent updates, run `flatpak update --user` or use your desktop software center.

To add only the application source:

```sh
flatpak remote-add --user --if-not-exists fractal-sexta https://yamada-sexta.github.io/fractal-sexta/fractal-sexta.flatpakrepo
```

The [public signing key](data/fractal-sexta-signing-key.asc) is included in both descriptors.
Its fingerprint is `10B8F31D4403272122E3D96DAFC6E910B5EA6414`.

### Install alongside official Fractal

The official [Fractal on Flathub](https://flathub.org/apps/org.gnome.Fractal) uses
`org.gnome.Fractal`. Fractal Sexta uses `io.github.yamada_sexta.FractalSexta`, with a separate launcher,
settings, session data, and keyring entries. Both can be installed and run at the same time.
Sign in separately in the fork; existing Fractal accounts and data are not migrated.

### Local development build

The development manifest is `build-aux/io.github.yamada_sexta.FractalSexta.Devel.json` and installs
`io.github.yamada_sexta.FractalSexta.Devel`. See [the contributor guide](CONTRIBUTING.md).

### Runtime Dependencies

On top of the dependencies required at build time and checked by Meson, Fractal depends on the
following dependencies at runtime:

* xdg-desktop-portal and its backends: some functionalities are dependent on the following portals,
  and a permission will be asked when necessary, but Fractal should work without them:
  * Secret: this portal or a Secret Service is required, see [storing secrets](#storing-secrets).
  * Camera: scan QR codes during verification.
  * Location: send the user’s location in a conversation.
  * Settings: get the 12h/24h time format system preference.
  * Background: allow the Flatpak to receive notifications with its window closed.
* GStreamer plugins:
  * gst-plugin-gtk4 (gstgtk4): required to preview videos in the timeline and to present the output
    of the camera.
  * libgstpipewire with the `pipewiredeviceprovider`: used to list and access the cameras.

#### Storing secrets

Fractal doesn’t store your **password**, but it stores your **access token** and the **passphrase**
used to encrypt the database and the local cache.

The Fractal Flatpaks use the [Secret **Portal**](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.portal.Secret.html)
to store those secrets. If you are using GNOME this should just work. If you are using a different
desktop environment or are facing issues, make sure `xdg-desktop-portal` is installed along with a
service that provides the [Secret portal backend interface](https://flatpak.github.io/xdg-desktop-portal/docs/doc-org.freedesktop.impl.portal.Secret.html),
like gnome-keyring or KWallet (since version 6.2).

Any version that is not sandboxed relies on software that implements the [Secret **Service** API](https://www.freedesktop.org/wiki/Specifications/secret-storage-spec/)
to store those secrets. Therefore, you need to have software providing that service on your system,
like gnome-keyring, pass with [pass_secret_service](https://github.com/mdellweg/pass_secret_service/),
or KWallet. Once again, if you are using GNOME this should just work.

If you prefer to use software that only implements the Secret Service API while using the Flatpaks,
you need to make sure that no service implementing the Secret portal backend interface is running,
and you need to allow Fractal to access the D-Bus service with this command:

```sh
flatpak override --user --talk-name=org.freedesktop.secrets io.github.yamada_sexta.FractalSexta
```

_For a local development build, use `io.github.yamada_sexta.FractalSexta.Devel`._

Or with [Flatseal](https://flathub.org/apps/details/com.github.tchx84.Flatseal), by adding
`org.freedesktop.secrets` in the **Session Bus** > **Talk** list of Fractal.

## Security Best Practices

You should use a strong **password** that is hard to guess to protect the secrets stored on your
device, whether the password is used directly to unlock your secrets (with a password manager for
example) or if it is used to open your user session and your secrets are unlocked automatically
(which is normally the case with a GNOME session).

Furthermore, make sure to lock your system when stepping away from the computer since an unlocked
computer can allow other people to access your private communications and your secrets.

## Contributing

### Code

Please follow our [contributing guidelines](CONTRIBUTING.md).

### Translations

Fractal is translated by the GNOME translation team on [Damned lies](https://l10n.gnome.org/).

Find your language in the list on [the Fractal module page on Damned lies](https://l10n.gnome.org/module/fractal/).

The names of the emoji displayed during verification come from [the Matrix specification repository](https://github.com/matrix-org/matrix-spec/tree/main/data-definitions).
They are translated on [Element’s translation platform](https://translate.element.io/projects/matrix-doc/sas-emoji-v1).

## Frequently Asked Questions

Does Fractal have encryption support?

: **Yes**, since Fractal 5, encryption is supported using Cross-Signing. See
  <https://gitlab.gnome.org/World/fractal/-/issues/717> for more info on the state of encryption.

Can I run Fractal with the window closed?

: **Yes.** On Linux, closing the window keeps Fractal Sexta running and receiving notifications.
  The Flatpak requests background permission the first time you close the window. If permission is
  denied or the Background portal is unavailable, the window stays open with an explanation.

  KDE shows a system tray indicator with **Open** and **Quit**. GNOME needs an AppIndicator /
  StatusNotifierItem extension to show it. Background operation also works without an indicator:
  reopen Fractal Sexta from the application launcher or a notification. **Ctrl+Q** quits the app;
  closing the window only hides it. The app does not start automatically at login.

## The origin of Fractal

The current version is a complete rewrite of Fractal built on top of the
[matrix-rust-sdk](https://github.com/matrix-org/matrix-rust-sdk) using [GTK4](https://gtk.org/).

The previous version of Fractal was using GTK3 and its own backend to talk to a matrix homeserver,
the code can be found in the [`legacy` branch](https://gitlab.gnome.org/World/fractal/-/tree/legacy).

Initial versions were based on Fest <https://github.com/fest-im/fest>, formerly called ruma-gtk.
In the origins of the project it was called guillotine, based on French revolution, in relation with
the Riot client name, but it's a negative name so we decide to change for a math one.

The name Fractal was proposed by Regina Bíró.

## Code of Conduct

Be nice to each other.

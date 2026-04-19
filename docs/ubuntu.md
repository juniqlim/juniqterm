# Ubuntu Install

This is the supported path for running growTerm on Ubuntu 24.04 UI.

## Install From Source

From the repository root:

```bash
./install-ubuntu.sh
```

The installer:

- installs Ubuntu build and graphics dependencies with `apt`
- installs Rust with `rustup` if `cargo` is missing
- builds `growterm` in release mode
- installs the binary to `~/.local/bin/growterm`
- creates `~/.local/share/applications/growterm.desktop`

## Build a Debian Package

For a normal Ubuntu app install experience, build a `.deb` package:

```bash
./package-ubuntu-deb.sh
```

The package is written to `target/packages/`.

Install it with:

```bash
sudo apt install ./target/packages/growterm_0.1.0_$(dpkg --print-architecture).deb
```

This path is better for end users because build tools and header packages are
only needed on the packaging machine, not on every machine that installs the
app.

## Run

From a terminal:

```bash
growterm
```

Or open `growTerm` from the desktop app launcher.

If `growterm` is not found, add `~/.local/bin` to `PATH`:

```bash
echo 'export PATH="$HOME/.local/bin:$PATH"' >> ~/.profile
. ~/.profile
```

## GNOME Notes

Ubuntu GNOME may run on Wayland or Xorg. Check with:

```bash
echo "$XDG_SESSION_TYPE"
```

If Wayland has input, clipboard, or GPU surface issues, log out and choose
`Ubuntu on Xorg` from the login screen gear menu.

## Verified

Verified on Ubuntu 24.04 ARM VM:

- `cargo check -p growterm-app`
- `cargo build -p growterm-app`
- `./package-ubuntu-deb.sh`
- `sudo apt install ./target/packages/growterm_0.1.0_arm64.deb`
- installed `/usr/bin/growterm` runs under a test X server
- `growterm` creates an X11 window under Xorg/Openbox

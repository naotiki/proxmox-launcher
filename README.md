# PVE VM Launcher

Ratatui-based TUI for launching Proxmox VM desktop consoles from a Proxmox VE host.

The app intentionally talks to Proxmox through local CLI commands (`qm` and `pvesh`) instead of storing API tokens.

## Requirements

- Proxmox VE host
- Root shell or equivalent sudo permissions for `qm` and `pvesh`
- `remmina` for VNC attach
- `remote-viewer` from `virt-viewer` for SPICE attach

```bash
apt update
apt install -y remmina virt-viewer
```

## Run

```bash
cargo build --release
sudo ./target/release/pve-vm-launcher
```

To inspect generated SPICE `.vv` or Remmina profile files, keep temporary files:

```bash
sudo ./target/release/pve-vm-launcher --keep-temp-files
ls -l /tmp/pve-vm-launcher/
```

Running again without `--keep-temp-files` cleans old generated files on startup.

## Configuration

Optional config file:

```text
~/.config/pve-vm-launcher/config.toml
```

The launcher creates this file with default values on first startup. When started with `sudo`, it prefers the invoking user's home directory from `SUDO_USER`, not `/root`.

Viewer command, extra arguments, and environment variables can be configured separately for SPICE and VNC:

```toml
[viewer.spice]
command = "remote-viewer"
args = ["--full-screen"]
run_as_invoking_user = true

[viewer.spice.env]
GDK_BACKEND = "x11"

[viewer.vnc]
command = "remmina"
args = []
run_as_invoking_user = true

[viewer.vnc.env]
GDK_BACKEND = "x11"
```

Generated file arguments are appended automatically. SPICE runs as `command <args> <file.vv>`, and VNC runs as `command <args> -c <profile.remmina>`.

When the launcher is started with `sudo`, viewers run as `SUDO_USER` by default. This keeps RDP/X11 session access with variables such as `DISPLAY`, `XAUTHORITY`, `DBUS_SESSION_BUS_ADDRESS`, and `XDG_RUNTIME_DIR` while keeping Proxmox CLI operations privileged. Set `run_as_invoking_user = false` for a viewer only if it must run as root.

## CI Artifact

GitHub Actions builds a Linux x64 release artifact on push, pull request, and manual workflow runs.

Download `pve-vm-launcher-linux-x64` from the workflow run artifacts. It contains:

- `pve-vm-launcher-x86_64-unknown-linux-gnu.tar.gz`
- `pve-vm-launcher-x86_64-unknown-linux-gnu.tar.gz.sha256`

## Key Bindings

| Key | Action |
| --- | --- |
| `j` / `k` or arrows | Move selection |
| `Enter` | Open action palette |
| `r` | Refresh VM list |
| `a` | Attach automatically, SPICE first then VNC |
| `p` | Attach via SPICE |
| `v` | Attach via VNC |
| `s` | Start selected VM |
| `S` | Shutdown selected VM |
| `f` | Force stop selected VM |
| `b` | Reboot selected VM |
| `x` | Reset selected VM |
| `l` | Open logs |
| `?` | Help |
| `q` | Quit |

## Notes

- VNC attach is experimental because Proxmox `vncproxy` is a short-lived proxy, not a regular persistent VNC server.
- SPICE is the preferred attach path when the VM display configuration supports it.
- Temporary `.vv` and Remmina profile files are created under `/tmp/pve-vm-launcher` with private permissions and scheduled for deletion after viewer launch.
- Command logs are written to `~/.local/state/pve-vm-launcher/app.log`.

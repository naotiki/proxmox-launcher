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

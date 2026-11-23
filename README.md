# wake

Wake the machines in the house with Wake-on-LAN.

```console
$ wake pc --wait
➜ pc       magic packet sent · 74:56:3C:B4:F5:4E → 192.168.50.255:9
⠼ pc       waiting for RDP on 192.168.50.180:3389 12s
✔ pc       awake after 23s · RDP answered
```

## Usage

```text
wake <TARGET>...         wake presets, `all`, or MAC addresses
wake pc -w [SECS]        ...and wait until it answers (default 120 s)
wake all                 wake every machine except this one
wake status              show which machines are up
wake list                show the presets
wake self-update         update to the latest release (--check to only look)
```

Put `-w` after the target (`wake pc -w`): `wake -w pc` reads `pc` as the
number of seconds.

`wake` checks each machine's service port first and skips machines that
already answer (`--force` sends anyway). The machine running `wake` is always
skipped, so `wake all` works the same from every machine.

Exit codes: `0` sent (or awake), `1` a machine didn't answer within `--wait`
or a send failed, `2` bad arguments, `130` interrupted.

## Machines

| Preset | Machine | MAC | Awake when |
|---|---|---|---|
| `pc` | Windows desktop (Totoro) | `74:56:3C:B4:F5:4E` | RDP answers on `192.168.50.180:3389` |
| `no-face` | Mac Studio | `BC:89:C1:E2:01:2B` | SSH answers on `192.168.50.80:22` |
| `calcifer` | Homeserver | `9C:6B:00:45:2B:C2`, `…:C1` | SSH answers on `192.168.50.100:28` |

The presets live in [`src/presets.rs`](src/presets.rs).

- **pc** wakes from sleep, hibernate, or shutdown over its wired Ethernet
  adapter (Wi-Fi can't receive the packet when it's off).
- **no-face** wakes only from sleep: Apple Silicon Macs can't be woken from a
  full shutdown.
- **calcifer** gets the packet on both bonded NICs' permanent MACs, because the
  bond's shared MAC is gone when the OS is off. If `WAKE_IPMI_USER` and
  `IPMI_PASSWORD` are set and `ipmitool` is installed, `wake calcifer` also
  asks its BMC (`192.168.50.50`) for `chassis power on`, which works even when
  Wake-on-LAN doesn't.

## Install

| Machine | How | Updates |
|---|---|---|
| NixOS | the flake: `inputs.wake.packages.${system}.default` | with the flake lock |
| macOS (Apple Silicon) | `curl -fsSL https://raw.githubusercontent.com/dantebarbieri/wake/main/install/install.sh \| sudo sh` | LaunchDaemon `com.danteb.wake-update`, daily 04:15 |
| Windows | `irm https://raw.githubusercontent.com/dantebarbieri/wake/main/install/install.ps1 \| iex` | scheduled task `wake self-update`, daily 04:15 and at logon |

Uninstall with `sudo sh install.sh --uninstall` or `install.ps1 -Uninstall`.

## Releasing

Bump `version` in `Cargo.toml` and push to `main`. The release workflow tags
`v<version>` and publishes `wake-<target>.tar.gz`/`.zip` for
`aarch64-apple-darwin`, `x86_64-pc-windows-msvc` and
`x86_64-unknown-linux-musl`; installed copies pick it up on their next daily
update.

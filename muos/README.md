# retsend for muOS

Transfer files between a muOS handheld and your phone or PC over wifi, using
[LocalSend](https://localsend.org).

Every device muOS runs on is aarch64 and carries its own SDL2, so this package is
the binary and a launcher — no bundled libraries. muOS also ships PortMaster, and
`retsend-portmaster.zip` installs the same app under Ports; this one puts it in
the **Applications** menu instead, where it belongs on a device with no ROM to
launch.

## Install

Copy `retsend-muos.muxapp` to `ARCHIVE/` on the SD card, then run
**Applications → Archive Manager** and pick it. It unpacks into
`MUOS/application/Retsend/` and appears under **Applications**.

## Setup

1. Install LocalSend on your phone or PC (localsend.org).
2. Connect both to the same network — wifi in muOS's own settings.
3. Launch Retsend — nearby devices appear on the radar.

Received files land in `ROMS/` on the card muOS boots from; change the folder in
Settings. The config lives in `MUOS/application/Retsend/data/config.toml`, and
the last run's log in `MUOS/application/Retsend/log.txt`.

## Controls

| Button  | Action                                                  |
|---------|---------------------------------------------------------|
| D-pad   | Navigate                                                |
| A       | Send to device / select file / accept / type (keyboard) |
| B       | Back / decline / cancel / erase (keyboard) · **quit**   |
| Start   | Settings · confirm send · OK (keyboard)                 |
| Select  | Refresh radar · switch roots · layer (keyboard)         |
| L1/R1   | Page through lists                                      |

## Notes for this device

**B on the home screen is the way out**, and here it has to be: muOS expects an
app to end itself, and the only exit it offers is `R2+SELECT+B`, which `kill -9`s
the foreground process — mid-transfer that leaves a peer holding a connection
that stopped answering. Leaving through B stops announcing and closes the sockets
first, so the peer sees a transfer end. The launcher sets `RETSEND_BACK_QUIT`;
every other port leaves the key inert, because there the launcher owns quitting.

Transfers are minutes with no button pressed, which is exactly what the device
reads as idle. The launcher holds `CAFFEINE` on for the run so it does not sleep
part-way through a file.

`SETUP_APP` from muOS's own `func.sh` does the work a launcher here would
otherwise repeat — the action file, the CPU governor, and the SDL controller
database that makes the pad arrive as a gamepad rather than as keys. It also
points `HOME` at the board's, which is read-only rootfs, so the launcher moves it
back to the app folder afterwards; the config, the log and the panic file all sit
on the card. Older muOS has no `SETUP_APP`, and the launcher spells the same
steps out when it is missing.

The app glyph is the package's own, at `glyph/retsend.png`. muOS looks in the
active theme first and falls back to this, so a theme carrying a `retsend` glyph
wins and one without still shows an icon.

It is 24×24 because that is the size themes cut their own glyphs to, and at the
usual "native" glyph setting muOS draws a raster at whatever pixels it has —
there is no fitting to the row. A vector would be fitted, but the `.svg` the
frontend reaches for did not render on device, so the package ships the raster
that does. `resources/retsend-glyph.svg` is what it is cut from.

## Credits

- Developed and ported by [mxmgorin](https://github.com/mxmgorin/)
- Implements the [LocalSend](https://localsend.org) protocol
- [muOS](https://muos.dev) is the MustardOS team's; this package only follows its
  application layout
- Source and issues: https://github.com/mxmgorin/retsend

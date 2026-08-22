# retsend for Allium

Transfer files between a Miyoo Mini Plus or Flip and your phone or PC over wifi, using
the [LocalSend](https://localsend.org).

**Miyoo Mini Plus and Flip only.** The original Mini has no wifi, so there is nothing for
this to talk over.

## Install

Unzip `retsend-allium.zip` into the root of the SD card, so that the app lands in
`Apps/Retsend.pak/`. It shows up on the Apps tab. The `.pak` suffix is what makes
a folder an app rather than one to walk into, so keep it if you rename anything.

## Setup

1. Install LocalSend on your phone or PC (localsend.org).
2. Connect both devices to the same wifi network — turn wifi on in Allium's
   settings first; it is off by default.
3. Launch Retsend — nearby devices appear on the radar.

Received files land in `/mnt/SDCARD/Roms` by default; change the folder in
Settings. The config lives in `Apps/Retsend.pak/data/config.toml`, and the last
run's log in `Apps/Retsend.pak/log.txt`.

## Controls

| Button  | Action                                                 |
|---------|--------------------------------------------------------|
| D-pad   | Navigate                                               |
| A       | Send to device / select file / accept / type (keyboard) |
| B       | Back / decline / cancel / erase (keyboard)              |
| Start   | Settings · confirm send · OK (keyboard)                 |
| Select  | Refresh radar · switch roots · layer (keyboard)         |
| L1/R1   | Page through lists                                     |
| MENU    | Quit                                                   |

MENU is Allium's key everywhere else on the device, and it does nothing for an
app — its in-game menu is RetroArch's — so here it is the way out. Held with
another button it stays Allium's: MENU+↑/↓ for brightness, MENU+←/→ for volume.

## Notes for this device

Allium keeps no kill helper of the kind OnionOS has, so the app answers MENU
itself and shuts the transfers down on its way out rather than being killed where
the frame stands. Powering off and closing the lid arrive as signals: the launcher
`exec`s the app so both land on it rather than on a shell holding it, and
`SIGTERM` stops announcing and closes the sockets before it goes — a peer sees a
transfer end rather than a connection that stopped answering.

The panel driver has no GPU behind it, so the launcher asks for the software
renderer and for the driver's one presentation path (`RETSEND_SOFTWARE`,
`RETSEND_BLIT`); the app bundles an SDL2 carrying that driver in `lib/`, as every
SDL2 port here does.

## Credits

- Developed and ported by [mxmgorin](https://github.com/mxmgorin/)
- Implements the [LocalSend](https://localsend.org) protocol
- Bundled SDL2 for the Miyoo Mini by
  [Steward Fu](https://github.com/steward-fu/sdl2) (zlib, with LGPL-2.1 drivers),
  built here — provenance and licences in `lib/README.md`
- [Allium](https://github.com/goweiwen/Allium) is goweiwen's; this package only
  follows its `Apps/*.pak` layout
- Source and issues: https://github.com/mxmgorin/retsend

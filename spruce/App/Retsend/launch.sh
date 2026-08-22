#!/bin/sh
# spruceOS (Miyoo Mini Plus / Mini Flip) launcher.
. /mnt/SDCARD/spruce/scripts/helperFunctions.sh
gamedir=$(cd "$(dirname "$0")" && pwd)
cd "$gamedir" || exit 1

# The binary and the bundled SDL are the Mini's; PyUI shows the entry anyway if
# `devices` in config.json ever falls out of step with a firmware rename.
if [ "$PLATFORM" != "MiyooMini" ]; then
  log_message "Retsend: this package is the Miyoo Mini build, but PLATFORM is $PLATFORM"
  exit 1
fi

# spruce only brings the radio up at boot, and only when this is set, so a card
# with wifi off reaches the radar with nothing on it and no hint why.
if [ "$(jq -r '.wifi // 0' "$SYSTEM_JSON" 2>/dev/null)" != "1" ]; then
  log_message "Retsend: wifi is off in spruce settings; nothing will be discovered"
fi

# Our SDL2 first, preloaded like every SDL2 port here: spruce carries a different
# build whose drivers answer to other names. The rest of the path is the one
# MiyooMini.cfg set, and `lib/fallback` goes last (lib/README.md).
export LD_LIBRARY_PATH="$gamedir/lib:$LD_LIBRARY_PATH:$gamedir/lib/fallback"
export LD_PRELOAD="$gamedir/lib/libSDL2-2.0.so.0"
export SDL_VIDEODRIVER=Mini
export EGL_VIDEODRIVER=Mini
# SDL lists its own `software` driver ahead of the panel's, and what that one
# draws never reaches the screen, so name the panel's outright.
export SDL_RENDER_DRIVER="Miyoo Mini"
# No SDL_AUDIODRIVER: a file transfer never opens the audio subsystem.

# The stock HOME is read-only rootfs; keep writable paths on the card.
export HOME="$gamedir"
export RETSEND_DATA_DIR="$gamedir/data"
export RETSEND_SAVE_DIR=/mnt/SDCARD/Roms
export RETSEND_PANIC_FILE="$gamedir/retsend-panic.log"
export RETSEND_SOFTWARE=1 # no GPU on the SSD202
export RETSEND_BLIT=1     # the panel driver shows texture copies and nothing else
# The pad layout follows the video driver's name, which this build spells `Mini`.
export RETSEND_KEYMAP=miyoo
# MENU reaches spruce's watchdog, but its action returns early for anything
# launched out of App/ rather than Emu/, so the key is ours and the way out.
export RETSEND_MENU_QUIT=1
#export RETSEND_LOG_LEVEL=debug

# `exec`: principal.sh waits on this script, and a shell holding the app would
# take a signal in its place — SIGTERM is what closes the sockets cleanly.
exec ./retsend >log.txt 2>&1

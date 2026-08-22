#!/bin/sh
# Allium (Miyoo Mini Plus / Flip) launcher.
gamedir=$(cd "$(dirname "$0")" && pwd)
cd "$gamedir" || exit 1

# Our SDL2 first, preloaded like every SDL2 port here; the libmi_* are the
# firmware's. `lib/fallback` goes last: stubs for what an Onion card carries and
# this one does not (lib/README.md).
export LD_LIBRARY_PATH="$gamedir/lib:/mnt/SDCARD/miyoo/lib:/lib:/config/lib:/customer/lib:$gamedir/lib/fallback"
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
# Allium keeps no kill helper and does nothing with a bare MENU, so the key is
# ours. Held with a pad it stays Allium's: brightness, volume, screenshot.
export RETSEND_MENU_QUIT=1
#export RETSEND_LOG_LEVEL=debug

# `exec`: alliumd stops and terminates the process it tracks, and a shell waiting
# on a child would take both in the app's place.
exec ./retsend >log.txt 2>&1

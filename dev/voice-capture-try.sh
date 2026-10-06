#!/usr/bin/env bash
# Spike V-A, by hand: does voiced's PipeWire device work on this machine?
#
#   dev/voice-capture-try.sh            list the audio nodes and the one voiced would capture
#                                       (read-only: opens no stream, never the microphone)
#   dev/voice-capture-try.sh capture 5  OPEN THE REAL MICROPHONE for 5 s at 16 kHz mono S16 and
#                                       print frame sizes, jitter and the peak level; also checks
#                                       that monitor and sink nodes are refused
#   dev/voice-capture-try.sh play       play a 440 Hz tone at 24 kHz on the default output
#
# Say something loud during `capture`: the peak should be well above 0. A browser or a terminal
# audio indicator will show the microphone in use while it runs, and only then.
set -euo pipefail
cd "$(dirname "$0")/.."
exec cargo run --quiet -p voiced --example voice_capture_try -- "${@:-list}"

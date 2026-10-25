#!/bin/sh
# Applies 70-opennek.rules to a radio that is already plugged in, without a new login
set -e
if command -v udevadm >/dev/null 2>&1; then
    udevadm control --reload-rules || true
    udevadm trigger --subsystem-match=tty --action=change || true
fi

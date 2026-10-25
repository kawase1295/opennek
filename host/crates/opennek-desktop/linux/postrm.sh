#!/bin/sh
# 70-opennek.rules is gone; the radio goes back to the default permissions
set -e
if command -v udevadm >/dev/null 2>&1; then
    udevadm control --reload-rules || true
    udevadm trigger --subsystem-match=tty --action=change || true
fi

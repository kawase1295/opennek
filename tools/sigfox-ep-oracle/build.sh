#!/usr/bin/env bash
# Builds the reference that produced the test vectors in host/crates/sigfox/tests/{auth,downlink}.rs from sigfox-ep-lib.
#   tools/sigfox-ep-oracle/build.sh /path/to/sigfox-ep-lib    (git clone https://github.com/sigfox-tech-radio/sigfox-ep-lib)
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
lib="${1:?path to sigfox-ep-lib}"
gcc -O1 -I"$here" -I"$lib/inc" -o "$here/oracle" "$here/oracle.c" \
  "$lib/src/core/sigfox_ep_bitstream.c" "$lib/src/core/sigfox_crc.c" "$lib/src/core/TI_aes_128_encr_only.c" \
  "$lib/src/sigfox_types.c" "$lib/src/sigfox_error.c"
echo "built $here/oracle"

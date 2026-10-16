// Reference vectors from sigfox-ep-lib. Usage:
//   oracle ul <id8> <counter> <rank> <key:pub|priv> <bf> <payloadhex|bit0|bit1|empty>  -> bitstream hex
//   oracle dl <id8> <counter> <key:pub|priv> <phy15hex>                               -> payload hex or INVALID
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include "sigfox_types.h"
#include "core/sigfox_ep_bitstream.h"
#include "manuf/mcu_api.h"
static const sfx_u8 PRIV[16] = {0x3f,0x2a,0x10,0x99,0x01,0x02,0x03,0x04,0x05,0x06,0x07,0x08,0x09,0x0a,0x0b,0x0c};
MCU_API_status_t MCU_API_get_ep_key(sfx_u8 *k, sfx_u8 n) { memcpy(k, PRIV, n); return MCU_API_SUCCESS; }
static int hexb(const char *s, sfx_u8 *out) { int n = strlen(s) / 2; for (int i = 0; i < n; i++) { unsigned v; sscanf(s + 2*i, "%2x", &v); out[i] = v; } return n; }
int main(int argc, char **argv) {
  sfx_u8 id[4]; hexb(argv[2], id); // MSB first, as AT$ID prints
  if (!strcmp(argv[1], "ul")) {
    SIGFOX_EP_BITSTREAM_application_frame_t in; memset(&in, 0, sizeof in);
    sfx_u8 payload[12]; sfx_u8 bs[32]; sfx_u8 size = 0;
    in.common_parameters.ep_id = id;
    in.common_parameters.message_counter = atoi(argv[3]);
    in.common_parameters.message_counter_rollover = 4096;
    in.common_parameters.ul_frame_rank = atoi(argv[4]);
    in.common_parameters.ep_key_type = !strcmp(argv[5], "pub") ? SIGFOX_EP_KEY_PUBLIC : SIGFOX_EP_KEY_PRIVATE;
    in.bidirectional_flag = atoi(argv[6]);
    if (!strcmp(argv[7], "bit0")) in.message_type = SIGFOX_APPLICATION_MESSAGE_TYPE_BIT0;
    else if (!strcmp(argv[7], "bit1")) in.message_type = SIGFOX_APPLICATION_MESSAGE_TYPE_BIT1;
    else if (!strcmp(argv[7], "empty")) in.message_type = SIGFOX_APPLICATION_MESSAGE_TYPE_EMPTY;
    else { in.message_type = SIGFOX_APPLICATION_MESSAGE_TYPE_BYTE_ARRAY; in.ul_payload_size_bytes = hexb(argv[7], payload); in.ul_payload = payload; }
    SIGFOX_EP_BITSTREAM_status_t st = SIGFOX_EP_BITSTREAM_build_application_frame(&in, bs, &size);
    if (st != SIGFOX_EP_BITSTREAM_SUCCESS) { printf("ERROR %d\n", st); return 1; }
    for (int i = 0; i < size; i++) printf("%02x", bs[i]); printf("\n");
  } else {
    SIGFOX_EP_BITSTREAM_dl_frame_t in; memset(&in, 0, sizeof in);
    sfx_u8 phy[15]; sfx_u8 out[8]; sfx_bool valid = 0;
    hexb(argv[5], phy);
    in.dl_phy_content = phy; in.ep_id = id; in.message_counter = atoi(argv[3]);
    in.ep_key_type = !strcmp(argv[4], "pub") ? SIGFOX_EP_KEY_PUBLIC : SIGFOX_EP_KEY_PRIVATE;
    SIGFOX_EP_BITSTREAM_decode_downlink_frame(&in, &valid, out);
    if (!valid) { printf("INVALID\n"); return 0; }
    for (int i = 0; i < 8; i++) printf("%02x", out[i]); printf("\n");
  }
  return 0;
}

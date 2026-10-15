#include "OpenNekRadio.h"

#include <WiFi.h>
#include <esp_now.h>
#include <esp_wifi.h>

namespace onek {

Radio radio;
const uint8_t Radio::BROADCAST[6] = {0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF};

static QueueHandle_t rxQueue;
static SemaphoreHandle_t sentSem;
static volatile bool lastSendOk;

#if ESP_IDF_VERSION_MAJOR >= 5
static void onRecv(const esp_now_recv_info_t* info, const uint8_t* data, int len) {
  const uint8_t* mac = info->src_addr;
  int8_t rssi = info->rx_ctrl ? info->rx_ctrl->rssi : 0;
#else
// IDF 4 系の ESP-NOW は RSSI を渡さないので、プロミスキャスで直前の管理フレームの RSSI を拾う
static volatile int8_t lastMgmtRssi = 0;
static void onPromisc(void* buf, wifi_promiscuous_pkt_type_t type) {
  if (type == WIFI_PKT_MGMT) lastMgmtRssi = ((wifi_promiscuous_pkt_t*)buf)->rx_ctrl.rssi;
}
static void onRecv(const uint8_t* mac, const uint8_t* data, int len) {
  int8_t rssi = lastMgmtRssi;
#endif
  if (len <= 0 || len > (int)sizeof(Packet::data)) return;
  Packet p;
  memcpy(p.src, mac, 6);
  p.rssi = rssi;
  p.len = len;
  memcpy(p.data, data, len);
  xQueueSend(rxQueue, &p, 0);  // 溢れたら捨てる
}

#if ESP_IDF_VERSION >= ESP_IDF_VERSION_VAL(5, 5, 0)
static void onSent(const wifi_tx_info_t* info, esp_now_send_status_t status) {
#else
static void onSent(const uint8_t* mac, esp_now_send_status_t status) {
#endif
  lastSendOk = status == ESP_NOW_SEND_SUCCESS;
  xSemaphoreGive(sentSem);
}

bool Radio::begin(uint8_t channel) {
  rxQueue = xQueueCreate(16, sizeof(Packet));
  sentSem = xSemaphoreCreateBinary();
  WiFi.mode(WIFI_STA);
  WiFi.disconnect();
  esp_wifi_set_ps(WIFI_PS_NONE);  // 省電力だと受信を取りこぼす
#if ESP_IDF_VERSION_MAJOR < 5
  wifi_promiscuous_filter_t filter = {.filter_mask = WIFI_PROMIS_FILTER_MASK_MGMT};
  esp_wifi_set_promiscuous_filter(&filter);
  esp_wifi_set_promiscuous_rx_cb(onPromisc);
  esp_wifi_set_promiscuous(true);
#endif
  if (!setChannel(channel)) return false;
  if (esp_now_init() != ESP_OK) return false;
  esp_now_register_recv_cb(onRecv);
  esp_now_register_send_cb(onSent);
  esp_now_peer_info_t peer = {};
  memcpy(peer.peer_addr, BROADCAST, 6);
  peer.channel = 0;  // 現在のチャンネル
  peer.ifidx = WIFI_IF_STA;
  return esp_now_add_peer(&peer) == ESP_OK;
}

bool Radio::setChannel(uint8_t channel) {
  if (channel < 1 || channel > 13) return false;
  if (esp_wifi_set_channel(channel, WIFI_SECOND_CHAN_NONE) != ESP_OK) return false;
  channel_ = channel;
  return true;
}

bool Radio::send(const uint8_t dst[6], const uint8_t* data, size_t len) {
  bool unicast = memcmp(dst, BROADCAST, 6) != 0;
  bool added = false;
  if (unicast && !esp_now_is_peer_exist(dst)) {
    esp_now_peer_info_t peer = {};
    memcpy(peer.peer_addr, dst, 6);
    peer.channel = 0;
    peer.ifidx = WIFI_IF_STA;
    if (esp_now_add_peer(&peer) != ESP_OK) return false;
    added = true;
  }
  xSemaphoreTake(sentSem, 0);
  bool ok = esp_now_send(dst, data, len) == ESP_OK && xSemaphoreTake(sentSem, pdMS_TO_TICKS(500)) == pdTRUE && lastSendOk;
  if (added) esp_now_del_peer(dst);
  return ok;
}

bool Radio::receive(Packet& out, TickType_t wait) { return xQueueReceive(rxQueue, &out, wait) == pdTRUE; }

void Radio::flush() { xQueueReset(rxQueue); }

void Radio::macAddress(uint8_t out[6]) const { esp_wifi_get_mac(WIFI_IF_STA, out); }

void formatMac(const uint8_t mac[6], char out[18]) {
  snprintf(out, 18, "%02X:%02X:%02X:%02X:%02X:%02X", mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]);
}

bool parseMac(const char* s, uint8_t out[6]) {
  unsigned v[6];
  if (sscanf(s, "%2x:%2x:%2x:%2x:%2x:%2x", &v[0], &v[1], &v[2], &v[3], &v[4], &v[5]) != 6) return false;
  for (int i = 0; i < 6; i++) out[i] = v[i];
  return true;
}

}  // namespace onek

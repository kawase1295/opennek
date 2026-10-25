// Generated from the host's Rust types by `UPDATE_API_TYPES=1 cargo test -p opennek-core api_types`.
// Do not edit: change the Rust types and regenerate.

export type Settings = { downlinkTimeoutMs: number, 
/**
 * Show and forward messages of devices that are not registered
 */
acceptUnknownDevices: boolean, 
/**
 * Port of the receiver
 */
sigfoxPort: ReceiverPort, 
/**
 * RC3 uplink centre frequency, Hz
 */
sigfoxFreq: number, 
/**
 * Transmit downlinks to real Sigfox devices. It puts a signal on air, so it is off until enabled
 */
sigfoxDownlink: boolean, 
/**
 * Language of the web UI; None until one is chosen, and the UI follows the browser's language
 */
language: Language | null, 
/**
 * Light or dark web UI
 */
theme: Theme, };

export type ReceiverPort = string | null;

export type Language = "en" | "ja";

export type Theme = "system" | "light" | "dark";

export type Device = { id: string, name: string, 
/**
 * 32 hex digits, or empty when not known
 */
key: string, downlinkMode: "none" | "static" | "callback", downlinkData: string, payloadConfig: string, createdAt: number, lastSeen: number | null, lastSeq: number | null, };

export type CallbackInput = { name: string, enabled: boolean, 
/**
 * Comma-separated device IDs; empty for every device
 */
devices: string, 
/**
 * GET, POST or PUT
 */
method: "GET" | "POST" | "PUT", url: string, 
/**
 * Header values may use variables
 */
headers: Record<string, unknown>, contentType: string, body: string, 
/**
 * Take downlink data from the response
 */
downlink: boolean, };

export type Callback = { id: number, name: string, enabled: boolean, 
/**
 * Comma-separated device IDs; empty for every device
 */
devices: string, 
/**
 * GET, POST or PUT
 */
method: "GET" | "POST" | "PUT", url: string, 
/**
 * Header values may use variables
 */
headers: Record<string, unknown>, contentType: string, body: string, 
/**
 * Take downlink data from the response
 */
downlink: boolean, };

export type Auth = "ok" | "bad" | "public" | "unverified";

export type DownlinkStatus = "none" | "pending" | "sent" | "acked" | "failed" | "nodata" | "unsupported";

export type Message = { id: number, deviceId: string, 
/**
 * Wall-clock ms
 */
time: number, seq: number, 
/**
 * Payload as hex
 */
data: string, 
/**
 * The device asked for a downlink
 */
ack: boolean, 
/**
 * A control message (keep-alive, downlink confirmation)
 */
oob: boolean, rssi: number, nbFrames: number, auth: Auth, station: string, decoded: Record<string, unknown> | null, downlink: string | null, downlinkStatus: DownlinkStatus, };

export type CallbackLog = { id: number, callbackId: number, messageId: number, time: number, status: number | null, ok: boolean, error: string | null, durationMs: number, response: string | null, };

export type CallbackResult = { status: number | null, ok: boolean, error: string | null, durationMs: number, response: string | null, 
/**
 * 8 bytes of hex, only when the callback asks for downlink data and the response has it
 */
downlink: string | null, };

export type RadioState = "off" | "disconnected" | "connecting" | "connected";

export type RadioStatus = { state: RadioState, port: string | null, fw?: string, freq?: number, rate?: number, error?: string, 
/**
 * Frames with a valid CRC
 */
frames: number, badFrames: number, lastFrameAt?: number, lostLines: number, };

export type ConsoleLine = { line: string, 
/**
 * Wall-clock ms
 */
time: number, };

export type PortInfo = { path: string, label: string, 
/**
 * The XIAO ESP32S3 & Wio-SX1262 kit
 */
kit: boolean, };

export type Status = { version: string, radio: RadioStatus, settings: Settings, 
/**
 * Where the host writes its log, once it does
 */
logFile: string | null, };

export type TestResult = { 
/**
 * The message it was tried with
 */
message: Message, result: CallbackResult, };

export type Event = { "type": "message", message: Message, } | { "type": "radio", status: RadioStatus, } | { "type": "console", line: string, time: number, } | { "type": "callbackLog", log: CallbackLog, };

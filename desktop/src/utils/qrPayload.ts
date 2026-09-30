import { UnifiedSharePayload } from "../types/rust";

// Helper utilities for encoding and decoding QR payloads.
export function encodeRoomQr(code: string): string {
  const trimmed = code.trim().toUpperCase();
  return `plenum://room/${encodeURIComponent(trimmed)}`;
}

export function encodePinQr(pin: string): string {
  const trimmed = pin.trim();
  return `plenum://pin/${encodeURIComponent(trimmed)}`;
}

export function encodeUnifiedQr(payload: UnifiedSharePayload): string {
  const params = new URLSearchParams();
  params.set("v", String(payload.version || 1));
  params.set("r", payload.room_code.trim().toUpperCase());
  if (payload.lan_ips && payload.lan_ips.length > 0) {
    params.set("ip", payload.lan_ips.join(","));
  }
  if (payload.port && payload.port > 0) {
    params.set("p", String(payload.port));
  }
  if (payload.pin && payload.pin.trim()) {
    params.set("pin", payload.pin.trim());
  }
  if (payload.peer_id && payload.peer_id.trim()) {
    params.set("id", payload.peer_id.trim());
  }
  if (payload.device_name && payload.device_name.trim()) {
    params.set("n", payload.device_name.trim());
  }
  return `plenum://v1/connect?${params.toString()}`;
}

export interface ParsedQrPayload {
  type: "room" | "pin" | "unified" | "raw";
  code: string;
  payload?: UnifiedSharePayload;
}

export function parseQrPayload(raw: string): ParsedQrPayload | null {
  const trimmed = raw.trim();
  if (!trimmed) return null;

  if (trimmed.startsWith("plenum://")) {
    const withoutPrefix = trimmed.slice("plenum://".length);

    // Check for query parameter format: plenum://v1/connect?... or plenum://connect?...
    const qIndex = withoutPrefix.indexOf("?");
    if (qIndex !== -1) {
      const queryStr = withoutPrefix.slice(qIndex + 1);
      const params = new URLSearchParams(queryStr);
      const room = (params.get("r") || params.get("room") || "").trim().toUpperCase();
      const ipStr = params.get("ip") || params.get("ips") || "";
      const lan_ips = ipStr ? ipStr.split(",").map(s => s.trim()).filter(Boolean) : [];
      const port = parseInt(params.get("p") || params.get("port") || "0", 10) || 0;
      const pin = (params.get("pin") || "").trim() || undefined;
      const peer_id = (params.get("id") || params.get("peer") || "").trim() || undefined;
      const device_name = (params.get("n") || params.get("name") || "").trim() || undefined;
      const version = parseInt(params.get("v") || "1", 10) || 1;

      if (room) {
        return {
          type: "unified",
          code: room,
          payload: {
            version,
            room_code: room,
            lan_ips,
            port,
            pin,
            peer_id,
            device_name,
          },
        };
      }
    }

    const slashIdx = withoutPrefix.indexOf("/");
    if (slashIdx === -1) {
      return null;
    }
    const type = withoutPrefix.slice(0, slashIdx).toLowerCase();
    const code = decodeURIComponent(withoutPrefix.slice(slashIdx + 1)).trim();
    if (!code) return null;

    if (type === "room") {
      const upper = code.toUpperCase();
      return {
        type: "room",
        code: upper,
        payload: {
          version: 1,
          room_code: upper,
          lan_ips: [],
          port: 0,
        },
      };
    }
    if (type === "pin") {
      return { type: "pin", code };
    }
    return null;
  }

  // Fallback for plain text codes (9 chars alphanumeric)
  const cleaned = trimmed.replace(/-/g, "").toUpperCase();
  if (/^[A-Z0-9]{9}$/.test(cleaned)) {
    return {
      type: "room",
      code: cleaned,
      payload: {
        version: 1,
        room_code: cleaned,
        lan_ips: [],
        port: 0,
      },
    };
  }

  return { type: "raw", code: trimmed };
}


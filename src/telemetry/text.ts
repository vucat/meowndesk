/** Curated text bounds mirrored from the Rust boundary. */

export const MESSAGE_MAX_BYTES = 160;
export const LABEL_MAX_BYTES = 128;
export const PROVIDER_MAX_BYTES = 32;
export const METRIC_ID_MAX_BYTES = 48;
export const MAX_SCOPES = 16;
export const MAX_CAPABILITIES = 64;
export const MAX_NETWORK_MEMBERS = 32;
export const MAX_NOTICES = 8;

const encoder = new TextEncoder();
const decoder = new TextDecoder('utf-8', { fatal: false });

export function utf8Length(text: string): number {
  return encoder.encode(text).length;
}

export function isWithin(text: string, maxBytes: number): boolean {
  return utf8Length(text) <= maxBytes;
}

/** Removes control characters (including line breaks) without trimming visible spaces. */
export function stripControl(text: string): string {
  return text.replace(/\p{Cc}/gu, '');
}

/** Truncates to at most `maxBytes`, never splitting a UTF-8 scalar value. */
export function truncateUtf8(text: string, maxBytes: number): string {
  const encoded = encoder.encode(text);
  if (encoded.length <= maxBytes) return text;
  let end = maxBytes;
  while (end > 0 && (encoded[end]! & 0xc0) === 0x80) end -= 1;
  return decoder.decode(encoded.subarray(0, end));
}

export function boundedLabel(text: string, maxBytes: number): string {
  return truncateUtf8(stripControl(text), maxBytes);
}

export function boundedMessage(text: string): string {
  return boundedLabel(text, MESSAGE_MAX_BYTES);
}

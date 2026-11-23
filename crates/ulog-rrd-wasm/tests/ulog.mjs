// A ULog writer for the tests: the header, then each message with its size and type.

export class Ulog {
  #parts = []

  constructor(startUs) {
    const header = new Uint8Array(16)
    header.set([0x55, 0x4c, 0x6f, 0x67, 0x01, 0x12, 0x35, 0x01])
    new DataView(header.buffer).setBigUint64(8, BigInt(startUs), true)
    this.#parts.push(header)
    this.message('B', new Uint8Array(40)) // flag bits, which must come first
  }

  message(kind, payload) {
    const head = new Uint8Array(3)
    new DataView(head.buffer).setUint16(0, payload.length, true)
    head[2] = kind.charCodeAt(0)
    this.#parts.push(head, payload)
  }

  format(definition) {
    this.message('F', new TextEncoder().encode(definition))
  }

  subscribe(msgId, multiId, name) {
    this.message('A', bytes([multiId], u16(msgId), new TextEncoder().encode(name)))
  }

  /** A data message: the timestamp, then `fields`, the message's bytes after it. */
  data(msgId, timestampUs, fields) {
    this.message('D', bytes(u16(msgId), u64(timestampUs), fields))
  }

  text(level, timestampUs, text) {
    this.message('L', bytes([0x30 + level], u64(timestampUs), new TextEncoder().encode(text)))
  }

  /** A multi-part info message; `isContinued` joins it to the one before it. */
  infoMulti(isContinued, key, value) {
    const k = new TextEncoder().encode(key)
    this.message('M', bytes([isContinued ? 1 : 0, k.length], k, value))
  }

  /** A parameter message: `key` is the type and the name, as in `float MPC_XY_VEL_MAX`; `value` its 4 bytes. */
  param(key, value) {
    const k = new TextEncoder().encode(key)
    this.message('P', bytes([k.length], k, value))
  }

  bytes() {
    return bytes(...this.#parts)
  }
}

export function bytes(...parts) {
  const out = new Uint8Array(parts.reduce((n, p) => n + p.length, 0))
  let at = 0
  for (const p of parts) {
    out.set(p, at)
    at += p.length
  }
  return out
}

export function u16(v) {
  const b = new Uint8Array(2)
  new DataView(b.buffer).setUint16(0, v, true)
  return b
}

export function u32(v) {
  const b = new Uint8Array(4)
  new DataView(b.buffer).setUint32(0, v, true)
  return b
}

export function u64(v) {
  const b = new Uint8Array(8)
  new DataView(b.buffer).setBigUint64(0, BigInt(v), true)
  return b
}

export function f32s(...values) {
  const b = new Uint8Array(4 * values.length)
  values.forEach((v, i) => new DataView(b.buffer).setFloat32(4 * i, v, true))
  return b
}

/** Three position samples, (1, 2, -3), (2, 2, -3) and (3, 2, -3), with an identity attitude. */
export function flight() {
  const ulog = new Ulog(1_000_000)
  ulog.format('vehicle_local_position:uint64_t timestamp;float x;float y;float z;float vx;float vy;float vz;')
  ulog.format('vehicle_attitude:uint64_t timestamp;float[4] q;')
  ulog.subscribe(0, 0, 'vehicle_local_position')
  ulog.subscribe(1, 0, 'vehicle_attitude')
  ulog.data(1, 1_000_000, f32s(1, 0, 0, 0))
  ;[1, 2, 3].forEach((x, i) => ulog.data(0, 1_000_000 + i * 100_000, f32s(x, 2, -3, 10, 0, 0)))
  return ulog
}

// The npm package's reader, against the built package in `pkg`: each log a recording holds answers
// Pulsar's `ParserLike` methods. Run with `node --test crates/ulog-rrd-wasm/tests/` after the build.

import assert from 'node:assert/strict'
import { readFile } from 'node:fs/promises'
import { before, test } from 'node:test'

import init, { convert, Options, read } from '../pkg/ulog_rrd.js'
import { bytes, flight, u16, u32, Ulog } from './ulog.mjs'

before(async () => {
  await init({ module_or_path: await readFile(new URL('../pkg/ulog_rrd_bg.wasm', import.meta.url)) })
})

/** The logs in the recording of `ulog`, converted with its series. */
function logs(ulog) {
  const options = new Options()
  options.scalars = true
  return read(convert(ulog.bytes(), options))
}

test('the npm reader answers one object per prefix, with Pulsar topic list and topic data shapes', () => {
  const got = logs(flight()).map((log) => {
    const topic = log.parseHeader().topics.find((t) => t.name === 'vehicle_local_position' && t.multiId === 0)
    const data = log.getTopicData('vehicle_local_position', 0)
    return {
      multiId: topic?.multiId,
      messageCount: topic?.messageCount,
      fields: topic?.fields,
      timestamp: data?.timestamp instanceof Float64Array ? [...data.timestamp] : null,
      x: data?.columns?.x instanceof Float64Array ? [...data.columns.x] : null,
    }
  })
  assert.deepEqual(got, [
    {
      multiId: 0,
      messageCount: 3,
      fields: ['x', 'y', 'z', 'vx', 'vy', 'vz'],
      timestamp: [1_000_000, 1_100_000, 1_200_000],
      x: [1, 2, 3],
    },
  ])
})

test('the npm reader names array fields as Pulsar does', () => {
  const got = logs(flight()).map((log) => {
    const topic = log.parseHeader().topics.find((t) => t.name === 'vehicle_attitude')
    return { fields: topic?.fields, arrayFields: topic?.arrayFields }
  })
  assert.deepEqual(got, [{ fields: ['q_0', 'q_1', 'q_2', 'q_3'], arrayFields: ['q'] }])
})

test("the npm reader's methods with no data return empty values and never throw", () => {
  const got = logs(flight()).map((log) => [log.getParameters(), log.getChangedParameters(), log.getParameter('X')])
  assert.deepEqual(got, [[{}, {}, undefined]])
})

test("the npm reader answers PX4's events in Pulsar's shape", () => {
  const ulog = new Ulog(1_000_000)
  ulog.format('event:uint64_t timestamp;uint32_t id;uint16_t event_sequence;uint8_t[25] arguments;uint8_t log_levels;')
  ulog.subscribe(0, 0, 'event')
  const args = new Uint8Array(25)
  args.set([1, 2, 3])
  ulog.data(0, 1_050_000, bytes(u32(7), u16(0), args, [0x44]))
  const got = logs(ulog).map((log) =>
    log.getEvents().map((e) => ({
      timestamp: e.timestamp,
      id: e.id,
      logLevel: e.logLevel,
      args: e.args instanceof Uint8Array ? [...e.args] : null,
    })),
  )
  assert.deepEqual(got, [
    [{ timestamp: 1_050_000, id: 7, logLevel: 4, args: [1, 2, 3, ...new Array(22).fill(0)] }],
  ])
})

test("the npm reader answers the firmware's event definitions", () => {
  const withBlob = flight()
  withBlob.infoMulti(false, 'uint8_t[3] metadata_events', new Uint8Array([1, 2, 3]))
  withBlob.infoMulti(true, 'uint8_t[2] metadata_events', new Uint8Array([4, 5]))
  const blob = (log) => {
    const b = log.getEventMetadataBlob()
    return b instanceof Uint8Array ? [...b] : b
  }
  assert.deepEqual([logs(withBlob).map(blob), logs(flight()).map(blob)], [[[1, 2, 3, 4, 5]], [null]])
})

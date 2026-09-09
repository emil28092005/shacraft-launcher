import { deepStrictEqual, equal, match } from 'node:assert/strict'
import { test } from 'node:test'
import { canRunUpdater, initialUpdaterState, updaterMutating, updaterPercent, updaterReducer } from './updater'
import { createUpdaterController } from '../services/updater'
import type { UpdaterApi } from '../services/updater'
import type { UpdaterStatus } from '../types/updater'

function status(phase: UpdaterStatus['phase'], revision = 0): UpdaterStatus {
  return { revision, installedVersion: '0.2.0', testBuild: false, packageFormat: 'development', phase, availableVersion: null, releaseNotes: null,
    downloadedBytes: 0, totalBytes: null, canRetry: false, message: null }
}

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (error: unknown) => void
  const promise = new Promise<T>((yes, no) => { resolve = yes; reject = no })
  return { promise, resolve, reject }
}

const flush = () => new Promise<void>((resolve) => setImmediate(resolve))

function fixture(overrides: Partial<UpdaterApi> = {}) {
  const calls = { status: 0, check: 0, install: 0, restart: 0, open: 0, disposed: 0 }
  const receivers: Array<(value: UpdaterStatus) => void> = []
  const api: UpdaterApi = {
    status: async () => { calls.status++; return status('idle') },
    check: async () => { calls.check++; return status('available', calls.check) },
    install: async () => { calls.install++; return status('ready', 50) },
    restart: async () => { calls.restart++ },
    open: async () => { calls.open++ },
    watch: (receive) => { receivers.push(receive); return { ready: Promise.resolve(), dispose: () => { calls.disposed++ } } },
    ...overrides,
  }
  return { calls, receivers, api, controller: createUpdaterController(api) }
}

test('newer native progress wins over an older invoke result; stale failures cannot finish another request', () => {
  const started = updaterReducer(initialUpdaterState, { type: 'begin', command: 'install', request: 1 })
  const progress = updaterReducer(started, { type: 'status', status: status('verifying', 4) })
  equal(updaterReducer(progress, { type: 'status', status: status('downloading', 3) }), progress)
  equal(updaterReducer(progress, { type: 'status', status: status('available', 4) }), progress)
  const failed = updaterReducer(progress, { type: 'failed', request: 1, error: 'Network interrupted' })
  const retry = updaterReducer(failed, { type: 'begin', command: 'check', request: 2 })
  equal(updaterReducer(retry, { type: 'failed', request: 1, error: 'Old error' }), retry)
  equal(updaterReducer(retry, { type: 'settled', request: 1 }), retry)
  equal(retry.error, null)
})

test('one startup check survives a StrictMode reconnect and never downloads or restarts', async () => {
  const { controller, calls } = fixture()
  const disconnect = controller.connect()
  disconnect() // StrictMode cleans up before listener registration has resolved.
  const disconnectRemount = controller.connect()
  await flush()
  equal(calls.check, 1)
  equal(calls.status, 1)
  equal(controller.snapshot().status?.phase, 'available')
  disconnectRemount()
  const disconnectAgain = controller.connect()
  await flush()
  equal(calls.check, 1)
  equal(calls.install, 0)
  equal(calls.restart, 0)
  disconnectAgain()
})

test('new pending account work or UI disposal before native handoff cancels an install request', async () => {
  const { controller, calls } = fixture()
  const disconnect = controller.connect()
  await flush()
  const blockedBeforeHandoff = controller.install()
  controller.setBlockedReason('Аккаунт сохраняется')
  equal(await blockedBeforeHandoff, false)
  equal(calls.install, 0)
  equal(controller.snapshot().pending, null)
  controller.setBlockedReason(null)
  const cancelledBeforeHandoff = controller.install()
  disconnect()
  equal(await cancelledBeforeHandoff, false)
  equal(calls.install, 0)
})

test('disconnect before event registration resolves cancels initialization and rejects old listener events', async () => {
  const ready = deferred<void>()
  let receive!: (value: UpdaterStatus) => void
  let disposed = 0
  const { controller, calls } = fixture({ watch: (callback) => {
    receive = callback
    return { ready: ready.promise, dispose: () => { disposed++ } }
  } })
  const disconnect = controller.connect()
  disconnect()
  ready.resolve()
  receive(status('available', 99))
  await flush()
  equal(disposed, 1)
  equal(calls.status, 0)
  equal(calls.check, 0)
  equal(controller.snapshot().status, null)
  equal(controller.snapshot().pending, null)
})

test('an earlier status read cannot hide an in-flight native installation', async () => {
  const read = deferred<UpdaterStatus>()
  const { controller, receivers, calls } = fixture({ status: () => read.promise })
  const disconnect = controller.connect()
  await flush()
  receivers[0]?.(status('installing', 6))
  read.resolve(status('idle', 0))
  await flush()
  equal(controller.snapshot().status?.phase, 'installing')
  equal(updaterMutating(controller.snapshot()), true)
  equal(calls.check, 0)
  equal(await controller.install(), false)
  disconnect()
})

test('startup check waits for pending work and explicit installation cannot overlap it or a double click', async () => {
  const installation = deferred<UpdaterStatus>()
  let installs = 0
  const { controller, calls } = fixture({ install: () => { installs++; return installation.promise } })
  controller.setBlockedReason('Сохраняем настройки')
  const disconnect = controller.connect()
  await flush()
  equal(calls.check, 0)
  controller.setBlockedReason(null)
  await flush()
  equal(calls.check, 1)
  controller.setBlockedReason('Игра запущена')
  equal(await controller.install(), false)
  equal(installs, 0)
  controller.setBlockedReason(null)
  const first = controller.install()
  equal(await controller.install(), false)
  await flush()
  equal(installs, 1)
  equal(updaterMutating(controller.snapshot()), true)
  installation.resolve(status('ready', 7))
  equal(await first, true)
  equal(calls.restart, 0) // The native install owns its restart; the UI never sends a second one.
  disconnect()
})

test('a failed automatic check releases its request; retry is explicit and never installs', async () => {
  let attempts = 0
  const { controller, calls } = fixture({ check: async () => {
    if (++attempts === 1) throw new Error('Network unavailable')
    return status('available', 3)
  } })
  const disconnect = controller.connect()
  await flush()
  match(controller.snapshot().error ?? '', /Network unavailable/)
  equal(controller.snapshot().pending, null)
  await flush()
  equal(attempts, 1)
  equal(await controller.check(), true)
  equal(controller.snapshot().error, null)
  equal(controller.snapshot().status?.phase, 'available')
  equal(calls.install, 0)
  disconnect()
})

test('failed event subscription can be retried without leaving a dead pending operation', async () => {
  let subscriptions = 0
  let disposed = 0
  const { controller, calls } = fixture({ watch: () => ({
    ready: ++subscriptions === 1 ? Promise.reject(new Error('Events unavailable')) : Promise.resolve(),
    dispose: () => { disposed++ },
  }) })
  const disconnect = controller.connect()
  await flush()
  match(controller.snapshot().error ?? '', /Events unavailable/)
  equal(calls.check, 0)
  equal(await controller.check(), true)
  equal(subscriptions, 2)
  equal(disposed, 1)
  equal(calls.check, 1)
  disconnect()
})

test('manual packages and a ready restart cannot accidentally invoke installation', () => {
  const manual = { ...initialUpdaterState, status: status('manual') }
  equal(canRunUpdater(manual, 'install'), false)
  equal(canRunUpdater(manual, 'open'), true)
  const ready = { ...initialUpdaterState, status: status('ready') }
  equal(canRunUpdater(ready, 'check'), false)
  equal(canRunUpdater(ready, 'install'), false)
  equal(canRunUpdater(ready, 'restart'), true)
  equal(canRunUpdater({ ...ready, blockedReason: 'Аккаунт сохраняется' }, 'restart'), false)
})

test('an indeterminate installer failure only permits the fixed manual recovery page', () => {
  const recovery = { ...initialUpdaterState, status: status('error'), blockedReason: 'Настройки не сохранены' }
  equal(canRunUpdater(recovery, 'check'), false)
  equal(canRunUpdater(recovery, 'install'), false)
  equal(canRunUpdater(recovery, 'restart'), false)
  equal(canRunUpdater(recovery, 'open'), true)
})

test('manual recovery can open the fixed page even when a failed settings save blocks mutations', async () => {
  const { controller, calls } = fixture({ status: async () => status('error', 2) })
  controller.setBlockedReason('Настройки не сохранены')
  const disconnect = controller.connect()
  await flush()
  equal(await controller.open(), true)
  equal(calls.open, 1)
  equal(calls.install, 0)
  equal(calls.check, 0)
  disconnect()
})

test('unknown download totals stay indeterminate and finite totals are bounded', () => {
  equal(updaterPercent(null), null)
  const downloading = status('downloading')
  equal(updaterPercent(downloading), null)
  equal(updaterPercent({ ...downloading, downloadedBytes: 10, totalBytes: 0 }), null)
  equal(updaterPercent({ ...downloading, downloadedBytes: NaN, totalBytes: 10 }), null)
  equal(updaterPercent({ ...downloading, downloadedBytes: 10, totalBytes: Infinity }), null)
  equal(updaterPercent({ ...downloading, downloadedBytes: -1, totalBytes: 10 }), null)
  deepStrictEqual([updaterPercent({ ...downloading, downloadedBytes: 7, totalBytes: 10 }),
    updaterPercent({ ...downloading, downloadedBytes: 11, totalBytes: 10 })], [70, 100])
})

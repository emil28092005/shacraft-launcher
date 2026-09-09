import { deepStrictEqual, equal, rejects } from 'node:assert/strict'
import { test } from 'node:test'
import { createSerialQueue, createSubscription, errorMessage, singleFlight } from './async.ts'

function deferred<T>() {
  let resolve!: (value: T) => void
  let reject!: (reason: unknown) => void
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise; reject = rejectPromise
  })
  return { promise, resolve, reject }
}

test('settings writes are serialized even while earlier requests are pending', async () => {
  const queue = createSerialQueue()
  const first = deferred<number>()
  const order: number[] = []
  const savedFirst = queue.enqueue(async () => { order.push(1); return first.promise })
  const savedSecond = queue.enqueue(async () => { order.push(2); return 2 })
  await Promise.resolve()
  deepStrictEqual(order, [1])
  first.resolve(1)
  equal(await savedFirst, 1)
  equal(await savedSecond, 2)
  deepStrictEqual(order, [1, 2])
})

test('a failed settings write does not poison later saves', async () => {
  const queue = createSerialQueue()
  const failed = queue.enqueue(async () => { throw new Error('disk full') })
  const retried = queue.enqueue(async () => 'saved')
  await rejects(failed, /disk full/)
  equal(await retried, 'saved')
  await queue.settled()
})

test('unmount before native registration still unregisters the late listener once', async () => {
  const registration = deferred<() => void>()
  let cleanupCount = 0
  const subscription = createSubscription([registration.promise])
  subscription.dispose()
  registration.resolve(() => { cleanupCount += 1 })
  await subscription.ready
  subscription.dispose()
  equal(cleanupCount, 1)
})

test('partial listener failure cleans up successful and late registrations', async () => {
  const failed = deferred<() => void>()
  const late = deferred<() => void>()
  let cleanupCount = 0
  const subscription = createSubscription([
    Promise.resolve(() => { cleanupCount += 1 }), failed.promise, late.promise,
  ])
  failed.reject(new Error('listen failed'))
  await rejects(subscription.ready, /listen failed/)
  equal(cleanupCount, 1)
  late.resolve(() => { cleanupCount += 1 })
  await Promise.resolve()
  equal(cleanupCount, 2)
  subscription.dispose()
  equal(cleanupCount, 2)
})

test('Rust string errors remain visible instead of being replaced with generic copy', () => {
  equal(errorMessage('Invalid signature', 'fallback'), 'Invalid signature')
  equal(errorMessage(new Error('Disk full'), 'fallback'), 'Disk full')
  equal(errorMessage(null, 'fallback'), 'fallback')
  equal(errorMessage('', 'fallback'), 'fallback')
})

test('overlapping account restores share one native request and do not cache the session', async () => {
  const first = deferred<string>()
  let calls = 0
  const restore = singleFlight(() => { calls += 1; return first.promise })
  const firstMount = restore()
  const strictModeRemount = restore()
  equal(firstMount, strictModeRemount)
  await Promise.resolve()
  equal(calls, 1)
  first.resolve('profile')
  equal(await strictModeRemount, 'profile')
  await restore()
  equal(calls, 2)
})

test('failed account restore can be retried', async () => {
  let calls = 0
  const restore = singleFlight(async () => {
    calls += 1
    if (calls === 1) throw new Error('network unavailable')
    return 'profile'
  })
  await rejects(restore(), /network unavailable/)
  equal(await restore(), 'profile')
  equal(calls, 2)
})

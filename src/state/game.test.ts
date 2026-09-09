import { deepStrictEqual, equal, match } from 'node:assert/strict'
import { test } from 'node:test'
import { gameReducer, initialGameState, installPercent } from './game.ts'

const profileId = 'aeronautics'
const installing = gameReducer(initialGameState, { type: 'install', profileId })
const launching = gameReducer(installing, { type: 'launch', profileId })

test('installation, running game and exit have distinct states', () => {
  const progressed = gameReducer(installing, { type: 'progress', progress: { stage: 'assets', currentBytes: 12, totalBytes: 24 } })
  equal(progressed.operation.phase, 'installing')
  const running = gameReducer(launching, { type: 'started', profileId })
  equal(running.operation.phase, 'running')
  deepStrictEqual(gameReducer(running, { type: 'exited', result: { profileId, exitCode: 0 } }), initialGameState)
})

test('a fast child exit cannot be overwritten by a late launch acknowledgement', () => {
  const exited = gameReducer(launching, { type: 'exited', result: { profileId, exitCode: 1 } })
  const lateAcknowledgement = gameReducer(exited, { type: 'started', profileId })
  equal(lateAcknowledgement.operation.phase, 'idle')
  match(lateAcknowledgement.error ?? '', /кодом 1/)
})

test('foreign exit events and late install progress cannot unlock a running game', () => {
  const running = gameReducer(launching, { type: 'started', profileId })
  equal(gameReducer(running, { type: 'exited', result: { profileId: 'other', exitCode: 0 } }), running)
  equal(gameReducer(running, { type: 'progress', progress: { stage: 'assets', currentBytes: 1, totalBytes: 1 } }), running)
  equal(gameReducer(running, { type: 'sync', profileId }), running)
})

test('sync completion does not complete a different operation', () => {
  equal(gameReducer(installing, { type: 'synced', profileId }), installing)
  const syncing = gameReducer(initialGameState, { type: 'sync', profileId })
  equal(gameReducer(syncing, { type: 'synced', profileId: 'other' }), syncing)
  equal(gameReducer(syncing, { type: 'synced', profileId }), initialGameState)
})

test('an operation failure releases the UI and a retry clears the error', () => {
  const failed = gameReducer(installing, { type: 'failed', error: 'Network failed' })
  equal(failed.operation.phase, 'idle')
  equal(failed.error, 'Network failed')
  equal(gameReducer(failed, { type: 'install', profileId }).error, null)
})

test('percent is bounded and unknown or invalid totals stay indeterminate', () => {
  equal(installPercent(null), null)
  equal(installPercent({ stage: 'java', currentBytes: 1, totalBytes: 0 }), null)
  equal(installPercent({ stage: 'assets', currentBytes: NaN, totalBytes: 10 }), null)
  equal(installPercent({ stage: 'assets', currentBytes: 15, totalBytes: 10 }), 100)
  equal(installPercent({ stage: 'assets', currentBytes: -5, totalBytes: 10 }), 0)
  equal(installPercent({ stage: 'assets', currentBytes: 5, totalBytes: 20 }), 25)
})

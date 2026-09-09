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

test('every launch can reconcile the modpack before installation and process spawn', () => {
  const syncing = gameReducer(initialGameState, { type: 'sync', profileId })
  equal(syncing.operation.phase, 'syncing')
  const synced = gameReducer(syncing, { type: 'synced', profileId })
  const installingAfterSync = gameReducer(synced, { type: 'install', profileId })
  equal(installingAfterSync.operation.phase, 'installing')
  const launchingAfterInstall = gameReducer(installingAfterSync, { type: 'launch', profileId })
  equal(launchingAfterInstall.operation.phase, 'launching')
  equal(gameReducer(launchingAfterInstall, { type: 'started', profileId }).operation.phase, 'running')
})

test('a fast child exit cannot be overwritten by a late launch acknowledgement', () => {
  const exited = gameReducer(launching, { type: 'exited', result: { profileId, exitCode: 1 } })
  const lateAcknowledgement = gameReducer(exited, { type: 'started', profileId })
  equal(lateAcknowledgement.operation.phase, 'idle')
  match(lateAcknowledgement.error ?? '', /кодом 1/)
})

test('a unified native launch can start directly from preparation, while repair only returns to idle', () => {
  equal(gameReducer(installing, { type: 'started', profileId }).operation.phase, 'running')
  deepStrictEqual(gameReducer(installing, { type: 'repaired', profileId }), initialGameState)
  const running = gameReducer(installing, { type: 'started', profileId })
  equal(gameReducer(running, { type: 'repaired', profileId }), running)
  equal(gameReducer(launching, { type: 'repaired', profileId }), launching)
})

test('a child exit received during preparation survives late progress and the native launch result', () => {
  for (const exitCode of [0, 1, null]) {
    const exited = gameReducer(installing, { type: 'exited', result: { profileId, exitCode } })
    equal(exited.operation.phase, 'idle')
    if (exitCode === 0) equal(exited.error, null)
    else match(exited.error ?? '', exitCode === null ? /без кода выхода/ : /кодом 1/)
    const lateProgress = gameReducer(exited, { type: 'progress', progress: { stage: 'launch', currentBytes: 0, totalBytes: 0 } })
    equal(lateProgress, exited)
    equal(gameReducer(lateProgress, { type: 'started', profileId }), exited)
    equal(gameReducer(exited, { type: 'repaired', profileId }), exited)
  }
})

test('completion and launch events for another profile cannot advance or release preparation', () => {
  equal(gameReducer(installing, { type: 'repaired', profileId: 'other' }), installing)
  equal(gameReducer(installing, { type: 'launch', profileId: 'other' }), installing)
  equal(gameReducer(installing, { type: 'started', profileId: 'other' }), installing)
  equal(gameReducer(installing, { type: 'exited', result: { profileId: 'other', exitCode: 0 } }), installing)
  equal(gameReducer(launching, { type: 'started', profileId: 'other' }), launching)
})

test('the native launch stage locks the launching state until a matching child event', () => {
  const nativeLaunching = gameReducer(installing, { type: 'progress', progress: { stage: 'launch', currentBytes: 0, totalBytes: 0 } })
  deepStrictEqual(nativeLaunching, launching)
  equal(gameReducer(nativeLaunching, { type: 'repaired', profileId }), nativeLaunching)
  equal(gameReducer(nativeLaunching, { type: 'install', profileId }), nativeLaunching)
  equal(gameReducer(nativeLaunching, { type: 'started', profileId }).operation.phase, 'running')
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

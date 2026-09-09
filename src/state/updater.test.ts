import { equal, match } from 'node:assert/strict'
import { test } from 'node:test'
import { initialUpdaterState, updateBlocksOperations, updatePercent, updaterReducer } from './updater.ts'

const available = updaterReducer(initialUpdaterState, { type: 'loaded', checked: true,
  status: { currentVersion: '0.1.3', supported: true, version: '0.1.4' } })
const downloading = updaterReducer(available, { type: 'install' })

test('checking and available update allow play; installation locks operations until restart', () => {
  equal(updateBlocksOperations(available), false)
  equal(updateBlocksOperations(updaterReducer(available, { type: 'check' })), false)
  equal(updateBlocksOperations(downloading), true)
  const ready = updaterReducer(downloading, { type: 'ready' })
  equal(updateBlocksOperations(ready), true)
  equal(updaterReducer(ready, { type: 'check' }), ready)
  equal(updaterReducer(ready, { type: 'install' }), ready)
  const restarting = updaterReducer(ready, { type: 'restart' })
  equal(updateBlocksOperations(restarting), true)
})

test('an installation failure permits playing and retrying; a restart failure retains the lock', () => {
  const failed = updaterReducer(downloading, { type: 'failed', error: 'Подпись не совпадает' })
  equal(updateBlocksOperations(failed), false)
  equal(failed.status?.version, '0.1.4')
  const retry = updaterReducer(failed, { type: 'install' })
  equal(retry.error, null)
  equal(retry.phase, 'downloading')
  const ready = updaterReducer(retry, { type: 'ready' })
  const restartError = updaterReducer(updaterReducer(ready, { type: 'restart' }), { type: 'failed', error: 'Перезапустите вручную' })
  equal(restartError.phase, 'ready')
  match(restartError.error ?? '', /вручную/)
  equal(updateBlocksOperations(restartError), true)
})

test('late progress cannot undo completion or a failed installation', () => {
  const installing = updaterReducer(downloading, { type: 'progress', progress: { stage: 'installing', downloadedBytes: 20 } })
  equal(updaterReducer(installing, { type: 'progress', progress: { stage: 'downloading', downloadedBytes: 10 } }), installing)
  const ready = updaterReducer(installing, { type: 'progress', progress: { stage: 'ready', downloadedBytes: 20 } })
  equal(updaterReducer(ready, { type: 'progress', progress: { stage: 'installing', downloadedBytes: 20 } }), ready)
  const failed = updaterReducer(downloading, { type: 'failed', error: 'Сбой сети' })
  equal(updaterReducer(failed, { type: 'ready' }), failed)
  equal(updaterReducer(failed, { type: 'progress', progress: { stage: 'ready', downloadedBytes: 20 } }), failed)
})

test('native status restores an update after a webview reload; early events preserve readiness', () => {
  for (const stage of ['downloading', 'installing', 'ready'] as const) {
    const restored = updaterReducer(initialUpdaterState, { type: 'loaded', checked: false,
      status: { ...available.status!, stage } })
    equal(restored.phase, stage)
    equal(updateBlocksOperations(restored), true)
  }
  const earlyReady = updaterReducer(initialUpdaterState, { type: 'progress', progress: { stage: 'ready', downloadedBytes: 20 } })
  const staleStatus = updaterReducer(earlyReady, { type: 'loaded', checked: false,
    status: { ...available.status!, stage: 'downloading' } })
  equal(staleStatus.phase, 'ready')
  equal(staleStatus.status?.currentVersion, '0.1.3')
})

test('unsupported package and no available version never enter installation', () => {
  const unsupported = updaterReducer(initialUpdaterState, { type: 'loaded', checked: false,
    status: { currentVersion: '0.1.3', supported: false, reason: 'Используйте AppImage' } })
  equal(updaterReducer(unsupported, { type: 'install' }), unsupported)
  const current = updaterReducer(initialUpdaterState, { type: 'loaded', checked: true,
    status: { currentVersion: '0.1.3', supported: true } })
  equal(updaterReducer(current, { type: 'install' }), current)
})

test('unknown, invalid and excessive progress cannot produce misleading percentages', () => {
  equal(updatePercent(null), null)
  equal(updatePercent({ stage: 'downloading', downloadedBytes: 1 }), null)
  equal(updatePercent({ stage: 'downloading', downloadedBytes: NaN, totalBytes: 20 }), null)
  equal(updatePercent({ stage: 'downloading', downloadedBytes: 1, totalBytes: Infinity }), null)
  equal(updatePercent({ stage: 'downloading', downloadedBytes: 10, totalBytes: 20 }), 50)
  equal(updatePercent({ stage: 'downloading', downloadedBytes: 30, totalBytes: 20 }), 100)
  equal(updatePercent({ stage: 'downloading', downloadedBytes: -1, totalBytes: 20 }), 0)
})

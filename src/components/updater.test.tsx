import { doesNotMatch, match } from 'node:assert/strict'
import { test } from 'node:test'
import { renderToStaticMarkup } from 'react-dom/server'
import { LauncherUpdateSettings } from './LauncherUpdateSettings'
import type { useLauncherUpdate } from '../hooks/useLauncherUpdate'
import { initialUpdaterState, updaterReducer, updateBlocksOperations, type UpdaterState } from '../state/updater'

function render(state: UpdaterState): string {
  const previous = Object.getOwnPropertyDescriptor(globalThis, 'window')
  const previousTauri = Object.getOwnPropertyDescriptor(globalThis, 'isTauri')
  Object.defineProperty(globalThis, 'window', { configurable: true, value: { isTauri: true } })
  Object.defineProperty(globalThis, 'isTauri', { configurable: true, value: true })
  try {
    const updater: ReturnType<typeof useLauncherUpdate> = {
      state, blocked: false, eventsReady: true, eventError: null,
      locksOperations: updateBlocksOperations(state), check: async () => {},
      install: async () => {}, restart: async () => {},
    }
    return renderToStaticMarkup(<LauncherUpdateSettings updater={updater} />)
  } finally {
    if (previous) Object.defineProperty(globalThis, 'window', previous)
    else Reflect.deleteProperty(globalThis, 'window')
    if (previousTauri) Object.defineProperty(globalThis, 'isTauri', previousTauri)
    else Reflect.deleteProperty(globalThis, 'isTauri')
  }
}

const available = updaterReducer(initialUpdaterState, { type: 'loaded', checked: true,
  status: { currentVersion: '0.1.4', supported: true, installationKind: 'deb', version: '0.1.5' } })

test('deb announces system authorization before installation without collecting credentials', () => {
  const html = render(available)
  match(html, /Для обновления deb потребуется подтверждение администратора в системном окне/)
  match(html, /class="update-primary"><[^>]+.*Обновить<\/button>/)
  doesNotMatch(html, /<input|type="password"|Перезапустить лаунчер/)
})

test('deb installation requests system confirmation and prevents duplicate install or check', () => {
  const downloading = updaterReducer(available, { type: 'install' })
  const installing = updaterReducer(downloading, { type: 'progress',
    progress: { stage: 'installing', downloadedBytes: 10, totalBytes: 10 } })
  const html = render(installing)
  match(html, /Подтвердите установку в системном окне и дождитесь завершения/)
  match(html, /aria-label="Установка обновления лаунчера"/)
  match(html, /class="update-primary" disabled=""/)
  match(html, /class="update-check" disabled=""/)
  doesNotMatch(html, /Для обновления deb потребуется|<input|type="password"/)
})

test('cancelled deb authorization remains visible and permits explicit retry without claiming success', () => {
  const downloading = updaterReducer(available, { type: 'install' })
  const installing = updaterReducer(downloading, { type: 'progress',
    progress: { stage: 'installing', downloadedBytes: 10 } })
  const cancelled = updaterReducer(installing, { type: 'failed', error: 'Установка отменена в системном окне.' })
  const html = render(cancelled)
  match(html, /role="status">Установка отменена в системном окне/)
  match(html, /class="update-primary">/)
  match(html, /Повторить проверку/)
  doesNotMatch(html, /disabled=""|Обновление установлено|Перезапустить лаунчер/)
  const retry = render(updaterReducer(cancelled, { type: 'install' }))
  match(retry, /Скачиваем обновление/)
  doesNotMatch(retry, /Установка отменена/)
})

test('AppImage and older native status retain their installation copy without administrator hints', () => {
  for (const installationKind of ['appimage', undefined] as const) {
    const status = { ...available.status!, installationKind }
    const downloading = updaterReducer({ ...available, status }, { type: 'install' })
    const installing = updaterReducer(downloading, { type: 'progress',
      progress: { stage: 'installing', downloadedBytes: 10 } })
    const html = render(installing)
    match(html, /Проверяем подпись и устанавливаем/)
    doesNotMatch(html, /администратора|системном окне/)
  }
})

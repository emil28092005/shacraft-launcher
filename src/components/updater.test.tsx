import { doesNotMatch, match } from 'node:assert/strict'
import { test } from 'node:test'
import { renderToStaticMarkup } from 'react-dom/server'
import { LauncherUpdate } from './LauncherUpdate'
import { initialUpdaterState } from '../state/updater'
import type { UpdaterState } from '../state/updater'
import type { UpdaterStatus } from '../types/updater'

function status(phase: UpdaterStatus['phase'], extra: Partial<UpdaterStatus> = {}): UpdaterStatus {
  return { revision: 1, installedVersion: '0.2.0', testBuild: false, packageFormat: 'development', phase, availableVersion: null, releaseNotes: null,
    downloadedBytes: 0, totalBytes: null, canRetry: false, message: null, ...extra }
}
function render(value: UpdaterStatus, extra: Partial<UpdaterState> = {}, native = true) {
  return renderToStaticMarkup(<LauncherUpdate state={{ ...initialUpdaterState, status: value, ...extra }} native={native}
    onCheck={() => {}} onInstall={() => {}} onRestart={() => {}} onOpenRelease={() => {}} />)
}

test('available updater notes are plain text and installation explicitly includes restart', () => {
  const html = render(status('available', { availableVersion: '0.3.0', releaseNotes: '<img src=x onerror="alert(1)">\n[Link](https://example.invalid)' }))
  match(html, /Версия 0\.2\.0/)
  match(html, /Новая версия: <strong>0\.3\.0/)
  match(html, /Установить и перезапустить/)
  match(html, /Установка закроет и перезапустит лаунчер/)
  match(html, /&lt;img src=x/)
  doesNotMatch(html, /<img|<a\s|dangerouslySetInnerHTML|authenticode|notarization/i)
})

test('an unconfigured build still reports its installed native version', () => {
  const html = render(status('unconfigured'))
  match(html, /Версия 0\.2\.0/)
  match(html, /Автообновление пока не настроено/)
  doesNotMatch(html, /Установить и перезапустить/)
})

test('test updater builds have a visible native-provided notice', () => {
  match(render(status('available', { testBuild: true })), /Тестовая сборка · тестовый канал обновлений/)
  doesNotMatch(render(status('available')), /Тестовая сборка/)
})

test('manual Linux packages offer release instructions without a native install button or frontend link', () => {
  const html = render(status('manual', { availableVersion: '0.3.0', packageFormat: 'deb' }))
  match(html, /Версия 0\.2\.0 · deb/)
  match(html, /системный менеджер пакетов/)
  match(html, /Открыть страницу выпусков/)
  doesNotMatch(html, /Установить и перезапустить|href=/)
})

test('pending game or settings work disables the explicit update action and explains why', () => {
  const html = render(status('available'), { blockedReason: 'Игра запущена' })
  match(html, /Игра запущена/)
  match(html, /<button[^>]*class="launcher-update-primary"[^>]*disabled=""/)
})

test('unknown download size does not announce a false percentage', () => {
  const html = render(status('downloading', { downloadedBytes: 1048576 }))
  match(html, /Скачано: 1\.0 МБ/)
  match(html, /role="progressbar"/)
  doesNotMatch(html, /aria-valuenow|Скачано: 0%/)
  match(render(status('downloading', { downloadedBytes: 50, totalBytes: 100 })), /aria-valuenow="50"/)
})

test('verification, installation and restart fallback remain distinct', () => {
  match(render(status('verifying')), /Проверяем подпись обновления/)
  match(render(status('installing')), /Устанавливаем обновление/)
  const ready = render(status('ready'), { error: 'Не удалось перезапустить' })
  match(ready, /Перезапустить лаунчер/)
  match(ready, /role="alert">Не удалось перезапустить/)
  doesNotMatch(ready, /Установить и перезапустить|Проверить обновления/)
})

test('check failures allow a clear retry, and an up-to-date result needs no install action', () => {
  match(render(status('error', { canRetry: true, message: 'Сеть недоступна' })), /Повторить проверку/)
  const current = render(status('no_update'))
  match(current, /Установлена актуальная версия/)
  doesNotMatch(current, /Установить и перезапустить/)
  const preview = render(status('available'), {}, false)
  match(preview, /Обновления доступны в приложении лаунчера/)
  match(preview, /class="launcher-update-primary" disabled=""/)
})

test('an indeterminate install failure offers manual recovery rather than retrying installation', () => {
  const html = render(status('error', { canRetry: false, message: 'Не удалось определить результат установки.' }))
  match(html, /Восстановите лаунчер из пакета/)
  match(html, /Открыть страницу выпусков/)
  doesNotMatch(html, /Повторить проверку|Установить и перезапустить|Перезапустить лаунчер/)
})

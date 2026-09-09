import { Download, RefreshCw } from 'lucide-react'
import { canRunUpdater, updaterPercent } from '../state/updater'
import type { UpdaterState } from '../state/updater'

export interface LauncherUpdateProps {
  state: UpdaterState
  native: boolean
  installedVersion?: string
  onCheck: () => void
  onInstall: () => void
  onRestart: () => void
  onOpenRelease: () => void
}

export function LauncherUpdate({ state, native, installedVersion, onCheck, onInstall, onRestart, onOpenRelease }: LauncherUpdateProps) {
  const status = state.status
  const phase = status?.phase
  const percent = updaterPercent(status)
  const downloadedBytes = status?.downloadedBytes ?? 0
  const error = state.error ?? (phase === 'error' ? status?.message : null)
  const needsRecovery = phase === 'error' && status?.canRetry === false
  const checking = state.pending?.command === 'check' || phase === 'checking' || state.pending?.command === 'status'
  const downloading = phase === 'downloading'
  const progressing = downloading || phase === 'verifying' || phase === 'installing' || state.pending?.command === 'install'
  const packageFormat = status?.packageFormat === 'development' ? 'для разработки'
    : status?.packageFormat === 'unpackaged' ? 'без установщика' : status?.packageFormat
  const titles = {
    idle: 'Можно проверить новую версию', checking: 'Проверяем обновления…',
    available: 'Доступно обновление', downloading: 'Скачиваем обновление…',
    verifying: 'Проверяем подпись обновления…', installing: 'Устанавливаем обновление…',
    ready: 'Обновление установлено — нужен перезапуск', no_update: 'Установлена актуальная версия',
    unconfigured: 'Автообновление пока не настроено', manual: 'Обновление пакета вручную', error: 'Не удалось обновить лаунчер',
  }
  return (
    <section className="launcher-update" aria-labelledby="launcher-update-title">
      <div className="launcher-update-heading"><h3 id="launcher-update-title">Лаунчер</h3>
        <span>Версия {status?.installedVersion ?? installedVersion ?? 'уточняется'}{packageFormat && ` · ${packageFormat}`}</span></div>
      {status?.testBuild && <p className="launcher-update-blocked">Тестовая сборка · тестовый канал обновлений</p>}
      <p className="launcher-update-status" role="status">{!native ? 'Обновления доступны в приложении лаунчера.'
        : phase ? titles[phase] : checking ? 'Читаем состояние обновления…' : 'Проверьте доступные обновления.'}</p>
      {status?.availableVersion && <p>Новая версия: <strong>{status.availableVersion}</strong></p>}
      {phase === 'manual' && <p>{status?.packageFormat === 'deb'
        ? 'Этот пакет обновляется вручную. Для установки .deb используйте системный менеджер пакетов; версия и инструкция доступны на странице выпусков.'
        : 'Эта сборка обновляется вручную. Выберите пакет для своей системы на странице выпусков.'}</p>}
      {phase === 'unconfigured' && <p>Для этой сборки канал обновлений недоступен. Проверка не меняет установленный лаунчер.</p>}
      {phase === 'available' && <p>Установка закроет и перезапустит лаунчер. Игру потребуется завершить, изменения настроек — сохранить.</p>}
      {phase === 'ready' && <p>Чтобы продолжить работу в новой версии, перезапустите лаунчер.</p>}
      {needsRecovery && <p>Автоматическое продолжение недоступно. Восстановите лаунчер из пакета на странице выпусков.</p>}
      {status?.message && phase !== 'error' && <p>{status.message}</p>}
      {status?.releaseNotes && <details className="launcher-update-notes"><summary>Что изменилось</summary><p tabIndex={0} aria-label="Описание изменений">{status.releaseNotes}</p></details>}
      {progressing && <div className="launcher-update-progress" role="progressbar" aria-label="Обновление лаунчера"
        aria-valuemin={0} aria-valuemax={100} aria-valuenow={downloading && percent !== null ? percent : undefined}>
        <i style={downloading && percent !== null ? { width: `${percent}%` } : undefined} />
      </div>}
      {downloading && <p>{percent === null
        ? `Скачано: ${(Math.max(0, Number.isFinite(downloadedBytes) ? downloadedBytes : 0) / 1024 / 1024).toFixed(1)} МБ`
        : `Скачано: ${percent}%`}</p>}
      {error && <p className="status-error" role="alert">{error}</p>}
      {state.blockedReason && native && <p className="launcher-update-blocked">{state.blockedReason}</p>}
      <div className="launcher-update-actions">
        {phase !== 'ready' && !needsRecovery && <button type="button" disabled={!native || !canRunUpdater(state, 'check')} onClick={onCheck}>
          <RefreshCw size={15} />{checking ? 'Проверяем…' : error ? 'Повторить проверку' : 'Проверить обновления'}
        </button>}
        {phase === 'available' && <button type="button" className="launcher-update-primary" disabled={!native || !canRunUpdater(state, 'install')} onClick={onInstall}>
          <Download size={15} />Установить и перезапустить
        </button>}
        {phase === 'ready' && <button type="button" className="launcher-update-primary" disabled={!native || !canRunUpdater(state, 'restart')} onClick={onRestart}>
          {state.pending?.command === 'restart' ? 'Перезапускаем…' : 'Перезапустить лаунчер'}
        </button>}
        {(phase === 'manual' || needsRecovery) && <button type="button" disabled={!native || !canRunUpdater(state, 'open')} onClick={onOpenRelease}>
          {state.pending?.command === 'open' ? 'Открываем…' : 'Открыть страницу выпусков'}
        </button>}
      </div>
    </section>
  )
}

import { ArrowDownToLine, RefreshCw } from 'lucide-react'
import type { useLauncherUpdate } from '../hooks/useLauncherUpdate'
import { isNative } from '../services/native'
import { updatePercent } from '../state/updater'

export function LauncherUpdateSettings({ updater }: { updater: ReturnType<typeof useLauncherUpdate> }) {
  const { state, blocked, eventsReady, eventError } = updater
  const { phase, status, error } = state
  const percent = updatePercent(state.progress)
  const working = phase === 'downloading' || phase === 'installing'
  const ready = phase === 'ready' || phase === 'restarting'
  const checking = phase === 'loading' || phase === 'checking'
  const deb = status?.installationKind === 'deb'

  return <section className="launcher-update" aria-labelledby="launcher-update-title">
    <div className="launcher-update-heading">
      <h3 id="launcher-update-title">ShaCraft Launcher</h3>
      {status?.currentVersion && <span>{status.currentVersion}</span>}
    </div>
    <div className="launcher-update-status" aria-live="polite">
      {!isNative() ? <p>Обновления доступны в приложении лаунчера.</p>
        : ready ? <p className="update-success">Обновление установлено. Перезапустите лаунчер.</p>
          : working ? <p>{phase === 'installing' ? deb
            ? 'Подтвердите установку в системном окне и дождитесь завершения.'
            : 'Проверяем подпись и устанавливаем…'
            : `Скачиваем обновление${percent === null ? '…' : ` · ${percent}%`}`}</p>
            : checking ? <p>Проверяем обновления…</p>
              : status?.supported === false ? <p>{status.reason || 'Для этой установки обновление доступно вручную на shacraft.ru/help#launcher.'}</p>
                : status?.version ? <p className="update-success">Доступна версия {status.version}</p>
                  : state.checked && !error ? <p>У вас последняя версия.</p>
                    : <p>Проверка новой версии лаунчера.</p>}
      {working && <progress aria-label={phase === 'installing' ? 'Установка обновления лаунчера' : 'Загрузка обновления лаунчера'} max={100} value={phase === 'installing' ? undefined : percent ?? undefined} />}
    </div>
    {status?.notes && status.version && !working && !ready && <details className="update-notes">
      <summary>Что нового</summary><p>{status.notes.slice(0, 1600)}</p>
    </details>}
    {error && <p className="status-error update-error" role="status">{error}</p>}
    {eventError && <p className="status-error update-error" role="status">{eventError} Перезапустите лаунчер, чтобы включить установку обновлений.</p>}
    {deb && status?.supported && status.version && !working && !ready &&
      <p className="update-hint">Для обновления deb потребуется подтверждение администратора в системном окне.</p>}
    {isNative() && <div className="update-actions">
      {ready ? <button className="update-primary" disabled={blocked || phase === 'restarting'} onClick={() => void updater.restart()}>
        <RefreshCw />{phase === 'restarting' ? 'Перезапускаем…' : 'Перезапустить лаунчер'}
      </button> : <>
        {status?.supported && status.version && <button className="update-primary"
          disabled={blocked || working || checking || !eventsReady} onClick={() => void updater.install()}>
          <ArrowDownToLine />{working ? 'Обновляем…' : 'Обновить'}
        </button>}
        {status?.supported !== false && <button className="update-check" disabled={checking || working} onClick={() => void updater.check()}>
          {checking ? 'Проверяем…' : error ? 'Повторить проверку' : 'Проверить обновления'}
        </button>}
      </>}
    </div>}
    {blocked && status?.supported && status.version && !working && <p className="update-hint">Завершите игру и текущие операции, чтобы обновить лаунчер.</p>}
  </section>
}

import { useEffect, useRef } from 'react'
import { ChevronRight, FolderOpen, LogOut, Users, Wrench, X } from 'lucide-react'
import type { useAccount } from '../hooks/useAccount'
import type { useSettings } from '../hooks/useSettings'
import type { JavaInstallation, NativeHost } from '../types/launcher'

interface SettingsDrawerProps {
  open: boolean
  locked: boolean
  host: NativeHost | null
  java: JavaInstallation | null | undefined
  preferences: ReturnType<typeof useSettings>
  session: ReturnType<typeof useAccount>
  onClose: () => void
}

export function SettingsDrawer({ open, locked, host, java, preferences, session, onClose }: SettingsDrawerProps) {
  const { settings, nickname, loaded, saving, error, nicknameError } = preferences
  const closeButton = useRef<HTMLButtonElement>(null)
  useEffect(() => {
    if (!open) return
    const previous = document.activeElement
    closeButton.current?.focus()
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onClose()
      if (event.key !== 'Tab') return
      const elements = closeButton.current?.closest('aside')?.querySelectorAll<HTMLElement>('button:not(:disabled), input:not(:disabled), select:not(:disabled)')
      const first = elements?.[0]
      const last = elements?.[elements.length - 1]
      if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last?.focus() }
      else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first?.focus() }
    }
    document.addEventListener('keydown', onKey)
    return () => {
      document.removeEventListener('keydown', onKey)
      if (previous instanceof HTMLElement) previous.focus()
    }
  }, [open, onClose])

  return (
    <>
      <div className={`drawer-backdrop ${open ? 'visible' : ''}`} onClick={onClose} />
      <aside className={`settings-drawer ${open ? 'open' : ''}`} inert={!open} aria-hidden={!open}
        role="dialog" aria-modal={open ? true : undefined} aria-labelledby="settings-title">
        <div className="drawer-title">
          <div><p>Настройки</p><h2 id="settings-title">Игра</h2></div>
          <button ref={closeButton} onClick={onClose} aria-label="Закрыть настройки"><X /></button>
        </div>
        <label className="range-setting">
          <span><strong>Оперативная память</strong><b>{settings.memoryMb / 1024} ГБ</b></span>
          <input type="range" min="3" max="12" step="1" value={settings.memoryMb / 1024} disabled={!loaded || locked}
            onChange={(event) => preferences.updateRam(Number(event.target.value))} />
          <small>Для Aeronautics рекомендуется 6 ГБ</small>
        </label>
        <div className="setting-row static"><span><Users />Аккаунт</span><small>{settings.accountMode === 'offline' ? 'Offline' : session.account?.name ?? 'Не авторизован'}</small></div>
        {settings.accountMode === 'offline' && (
          <label className="text-setting">
            <span><strong>Игровой ник</strong><small>Offline-профиль</small></span>
            <input value={nickname} maxLength={16} disabled={!loaded || locked} aria-invalid={!!nicknameError}
              aria-describedby="nickname-hint" onChange={(event) => preferences.updateNickname(event.target.value)}
              onBlur={preferences.saveNickname} placeholder="Player" />
            <small id="nickname-hint" className={nicknameError ? 'status-error' : undefined}>{nicknameError ?? 'Латинские буквы, цифры и _ · от 3 до 16 символов'}</small>
          </label>
        )}
        <label className="setting-row">
          <span>Тип аккаунта</span>
          <select value={settings.accountMode} disabled={!loaded || locked || saving || session.busy}
            onChange={(event) => preferences.updateMode(event.target.value === 'microsoft' ? 'microsoft' : 'offline')}>
            <option value="offline">Offline</option><option value="microsoft">Microsoft</option>
          </select>
        </label>
        {settings.accountMode === 'microsoft' && (
          <button className="setting-row" disabled={locked || saving || session.busy || session.account === undefined || !session.eventsReady}
            onClick={session.account ? session.logout : session.login}>
            <span><LogOut />{session.busy ? 'Ждём вход…' : session.account ? 'Выйти из Microsoft' : 'Войти через Microsoft'}</span>
          </button>
        )}
        <div className="setting-row static"><span><FolderOpen />Папка игры</span><small>{host ? 'В каталоге лаунчера' : 'Определяется…'}</small></div>
        <div className="setting-row static">
          <span><Wrench />Java</span>
          <small>{java === undefined ? host ? 'Проверяем…' : 'Проверяется в приложении'
            : java?.major === 21 ? 'Java 21 найдена' : java ? `Нужна Java 21 · найдена ${java.major}`
              : 'Лаунчер установит Java 21 автоматически'}</small>
        </div>
        <button className="setting-row" disabled title="Скоро"><span><Wrench />Дополнительные параметры</span><ChevronRight /></button>
        <div className="settings-feedback" aria-live="polite">
          {error && <><p className="status-error">{error}</p><button onClick={preferences.retry} disabled={locked || saving}>Повторить</button></>}
          {session.error && settings.accountMode === 'microsoft' && <p className="status-error">{session.error}</p>}
          {saving && <p>Сохраняем настройки…</p>}
        </div>
        <div className="drawer-note">{host ? `Данные лаунчера: ${host.dataDir}` : 'Java 21 будет управляться лаунчером автоматически.'}</div>
      </aside>
    </>
  )
}

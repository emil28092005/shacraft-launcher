import { useEffect, useRef } from 'react'
import { FolderOpen, Wrench, X } from 'lucide-react'
import { AccountSettings } from './AccountSettings'
import { LauncherUpdateSettings } from './LauncherUpdateSettings'
import type { useAccount } from '../hooks/useAccount'
import type { useSettings } from '../hooks/useSettings'
import type { useLauncherUpdate } from '../hooks/useLauncherUpdate'
import type { JavaInstallation, NativeHost } from '../types/launcher'

interface SettingsDrawerProps {
  open: boolean
  locked: boolean
  host: NativeHost | null
  java: JavaInstallation | null | undefined
  preferences: ReturnType<typeof useSettings>
  session: ReturnType<typeof useAccount>
  updater: ReturnType<typeof useLauncherUpdate>
  onClose: () => void
}

export function SettingsDrawer({ open, locked, host, java, preferences, session, updater, onClose }: SettingsDrawerProps) {
  const { settings, loaded, saving, error } = preferences
  const closeButton = useRef<HTMLButtonElement>(null)
  useEffect(() => {
    if (!open) return
    const previous = document.activeElement
    closeButton.current?.focus()
    const onKey = (event: KeyboardEvent) => {
      if (event.key === 'Escape') onClose()
      if (event.key !== 'Tab') return
      const elements = closeButton.current?.closest('aside')?.querySelectorAll<HTMLElement>('button:not(:disabled), input:not(:disabled), select:not(:disabled), summary')
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
          <div><p>Настройки</p><h2 id="settings-title">Лаунчер и игра</h2></div>
          <button ref={closeButton} onClick={onClose} aria-label="Закрыть настройки"><X /></button>
        </div>
        <LauncherUpdateSettings updater={updater} />
        <label className="range-setting">
          <span><strong>Оперативная память</strong><b>{settings.memoryMb / 1024} ГБ</b></span>
          <input type="range" min="3" max="12" step="1" value={settings.memoryMb / 1024} disabled={!loaded || locked}
            onChange={(event) => preferences.updateRam(Number(event.target.value))} />
          <small>Для Aeronautics рекомендуется 6 ГБ</small>
        </label>
        <AccountSettings session={session} locked={locked || saving} />
        <div className="setting-row static"><span><FolderOpen />Папка игры</span><small>{host ? 'В каталоге лаунчера' : 'Определяется…'}</small></div>
        <div className="setting-row static">
          <span><Wrench />Java</span>
          <small>{java === undefined ? host ? 'Проверяем…' : 'Проверяется в приложении'
            : java?.major === 21 ? 'Java 21 найдена' : java ? `Нужна Java 21 · найдена ${java.major}`
              : 'Лаунчер установит Java 21 автоматически'}</small>
        </div>
        <div className="settings-feedback" aria-live="polite">
          {error && <><p className="status-error">{error}</p><button onClick={preferences.retry} disabled={locked || saving}>Повторить</button></>}
          {saving && <p>Сохраняем настройки…</p>}
        </div>
        <div className="drawer-note">{host ? `Данные лаунчера: ${host.dataDir}` : 'Java 21 будет управляться лаунчером автоматически.'}</div>
      </aside>
    </>
  )
}

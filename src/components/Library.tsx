import { ChevronRight, Settings } from 'lucide-react'
import { servers } from '../data/servers'
import { linkedNickname } from '../state/account'
import type { ProfileState } from '../state/profiles'
import type { ShaCraftAccount, Server } from '../types/launcher'

interface LibraryProps {
  selected: Server
  profiles: Record<string, ProfileState>
  account: ShaCraftAccount | null | undefined
  locked: boolean
  native: boolean
  onSelect: (server: Server) => void
  onSettings: () => void
}

export function Library({ selected, profiles, account, locked, native, onSelect, onSettings }: LibraryProps) {
  const nickname = linkedNickname(account)
  const name = account === undefined ? 'Проверяем…' : account === null ? 'Не авторизован' : nickname ?? account.username

  return (
    <>
      <nav className="rail" aria-label="Настройки лаунчера">
        <button className="rail-button active" aria-label="Настройки" onClick={onSettings}><Settings /></button>
      </nav>
      <aside className="library-panel">
        <div className="library-heading"><p>Сборки</p><span>{servers.length} доступна</span></div>
        <div className="server-list">
          {servers.map((server) => {
            const profile = profiles[server.profileId]
            const status = !native ? 'Доступна' : !profile || profile.status === 'checking'
              ? 'Проверяем…' : profile.inspection?.upToDate ? 'Файлы проверены' : 'Требуется проверка'
            return (
              <button key={server.id} className={`server-row ${selected.id === server.id ? 'selected' : ''}`}
                aria-pressed={selected.id === server.id} disabled={locked} onClick={() => onSelect(server)}>
                <span className={`server-glyph ${server.id}`} aria-hidden="true">A</span>
                <span className="server-copy"><strong>{server.name}</strong><small>{status}</small></span>
                <ChevronRight size={16} />
              </button>
            )
          })}
        </div>
        <button className="account-chip" onClick={onSettings} aria-label="Аккаунт ShaCraft">
          <span className="avatar">{account ? (nickname ?? account.username).slice(0, 2).toUpperCase() : '?'}</span>
          <span><strong>{name}</strong><small>{account ? `ShaCraft · ${account.username}` : 'Войдите, чтобы играть'}</small></span>
          <ChevronRight size={16} />
        </button>
      </aside>
    </>
  )
}

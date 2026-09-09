import { ChevronRight, Library as LibraryIcon, LogOut, MessageCircle, Newspaper, Settings } from 'lucide-react'
import { servers } from '../data/servers'
import type { ProfileState } from '../state/profiles'
import type { LauncherSettings, MinecraftProfile, Server } from '../types/launcher'

interface LibraryProps {
  selected: Server
  profiles: Record<string, ProfileState>
  settings: LauncherSettings
  account: MinecraftProfile | null | undefined
  locked: boolean
  native: boolean
  onSelect: (server: Server) => void
  onSettings: () => void
  onLogout: () => void
}

export function Library(props: LibraryProps) {
  const { selected, profiles, settings, account, locked, onSelect, onSettings, onLogout } = props
  const name = settings.accountMode === 'offline' ? settings.nickname
    : account === undefined ? 'Проверяем…' : account?.name ?? 'Не авторизован'

  return (
    <>
      <nav className="rail" aria-label="Основное меню">
        <div className="rail-main">
          <button className="rail-button active" aria-label="Сборки" aria-current="page"><LibraryIcon /></button>
          <button className="rail-button" aria-label="Новости" disabled title="Скоро"><Newspaper /></button>
          <button className="rail-button" aria-label="Сообщество" disabled title="Скоро"><MessageCircle /></button>
        </div>
        <button className="rail-button" aria-label="Настройки" onClick={onSettings}><Settings /></button>
      </nav>
      <aside className="library-panel">
        <div className="library-heading"><p>Сборки</p><span>1 доступна</span></div>
        <div className="server-list">
          {servers.map((server) => {
            const profile = server.profileId ? profiles[server.profileId] : undefined
            const status = server.disabled ? 'На паузе' : !props.native ? 'Доступна'
              : !profile || profile.status === 'checking' ? 'Проверяем…'
                : profile.inspection?.upToDate ? 'Установлена' : 'Требуется проверка'
            return (
              <button key={server.id} className={`server-row ${selected.id === server.id ? 'selected' : ''}`}
                aria-pressed={selected.id === server.id} disabled={locked} onClick={() => onSelect(server)}>
                <span className={`server-glyph ${server.id}`} aria-hidden="true">{server.id === 'aoc' ? 'A' : 'C'}</span>
                <span className="server-copy"><strong>{server.name}</strong><small>{status}</small></span>
                <ChevronRight size={16} />
              </button>
            )
          })}
        </div>
        <div className="account-chip">
          <span className="avatar">{settings.accountMode === 'offline' || account ? name.slice(0, 2).toUpperCase() : '?'}</span>
          <span><strong>{name}</strong><small>{settings.accountMode === 'offline' ? 'Offline-аккаунт' : account ? 'Microsoft-аккаунт' : 'Войдите, чтобы играть'}</small></span>
          {settings.accountMode === 'microsoft' && account ? (
            <button className="account-logout" aria-label="Выйти из аккаунта" disabled={locked} onClick={onLogout}><LogOut size={16} /></button>
          ) : <ChevronRight size={16} />}
        </div>
      </aside>
    </>
  )
}

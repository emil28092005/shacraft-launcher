import { useState } from 'react'
import { LogOut, Users } from 'lucide-react'
import type { useAccount } from '../hooks/useAccount'
import { isNative } from '../services/native'

export function AccountSettings({ session, locked }: { session: ReturnType<typeof useAccount>; locked: boolean }) {
  const [username, setUsername] = useState('')
  const [password, setPassword] = useState('')
  const [registering, setRegistering] = useState(false)
  const [nickname, setNickname] = useState('')
  const { account, linkedNickname, busy } = session
  const disabled = locked || busy || account === undefined

  return (
    <>
      <div className="setting-row static"><span><Users />Аккаунт</span><small>{account === undefined ? 'Проверяем…' : account?.username ?? 'Не авторизован'}</small></div>
      {!account && (
        <form onSubmit={async (event) => {
          event.preventDefault()
          if (disabled) return
          if (await session.authenticate(username, password, registering)) setPassword('')
        }}>
          <label className="text-setting">
            <span><strong>Логин ShaCraft</strong><small>3–32 символа</small></span>
            <input value={username} minLength={3} maxLength={32} pattern="[A-Za-z0-9_]{3,32}" required autoComplete="username"
              disabled={disabled} onChange={(event) => setUsername(event.target.value)} placeholder="Логин" />
          </label>
          <label className="text-setting">
            <span><strong>Пароль</strong><small>Минимум 3 символа</small></span>
            <input type="password" value={password} minLength={3} maxLength={128} required
              autoComplete={registering ? 'new-password' : 'current-password'} disabled={disabled}
              onChange={(event) => setPassword(event.target.value)} placeholder="Пароль" />
          </label>
          <button className="setting-row" type="submit" disabled={disabled || !isNative()}>
            <span>{busy ? 'Подождите…' : registering ? 'Создать аккаунт' : 'Войти'}</span>
          </button>
          <button className="setting-row" type="button" disabled={disabled} onClick={() => { setRegistering(!registering); session.clearError() }}>
            <span>{registering ? 'Уже есть аккаунт' : 'Нет аккаунта — регистрация'}</span>
          </button>
          {!isNative() && <p className="account-hint">Вход и регистрация доступны в приложении лаунчера.</p>}
        </form>
      )}
      {account && !linkedNickname && (
        <form onSubmit={(event) => { event.preventDefault(); if (!disabled) void session.startLink(nickname) }}>
          <label className="text-setting">
            <span><strong>Игровой ник</strong><small>Aeronautics</small></span>
            <input value={nickname} minLength={3} maxLength={16} pattern="[A-Za-z0-9_]{3,16}" required
              disabled={disabled || session.linking} onChange={(event) => setNickname(event.target.value)} placeholder="Player" />
            <small>Подтвердите владение ником на сервере Aeronautics.</small>
          </label>
          <button className="setting-row" type="submit" disabled={disabled || session.linking}>
            <span>{session.linking ? 'Ожидаем подтверждения…' : 'Привязать ник'}</span>
          </button>
        </form>
      )}
      {account && linkedNickname && <div className="setting-row static"><span>Игровой ник</span><small>{linkedNickname}</small></div>}
      {session.linkMessage && <p className="account-hint" role="status">{session.linkMessage}</p>}
      {session.error && <p className="status-error account-hint" role="alert">{session.error}</p>}
      {account && <button className="setting-row" disabled={disabled} onClick={session.logout}><span><LogOut />Выйти из ShaCraft</span></button>}
    </>
  )
}

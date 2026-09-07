import React, { useEffect, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'
import {
  ChevronRight,
  Download,
  FolderOpen,
  Gauge,
  Globe2,
  LogOut,
  Minus,
  Play,
  RotateCcw,
  Settings,
  ShieldCheck,
  Square,
  Users,
  Wrench,
  X,
} from 'lucide-react'
import logo from './assets/shacraft-logo.png'
import './styles.css'

type Server = {
  id: string
  kicker: string
  name: string
  subtitle: string
  version: string
  profileId: string
}

type NativeHost = {
  platform: string
  dataDir: string
  launcherVersion: string
}

type NativeSettings = {
  memoryMb: number
  nickname: string
  accountMode: 'microsoft' | 'offline'
}

type JavaInstallation = {
  executable: string
  major: number
  version: string
}

type ProfileInspection = {
  managedFiles: number
  missingFiles: number
  mismatchedFiles: number
  upToDate: boolean
}

type SyncResult = {
  downloadedFiles: number
  reusedFiles: number
  downloadedBytes: number
}

type ShaCraftAccount = {
  username: string
  links: { server_id: string; mc_username: string }[]
}

type ShaCraftLoginResult = {
  account: ShaCraftAccount
  recoveryCodes: string[]
}

type ServerStatus = {
  online: number | null
  max: number | null
  reachable: boolean
}

type GameExitedPayload = {
  profileId: string
  exitCode: number | null
}

type InstallProgressPayload = {
  stage: 'java' | 'neoforge' | 'libraries' | 'assets'
  currentBytes: number
  totalBytes: number
}

const INSTALL_STAGE_LABEL: Record<InstallProgressPayload['stage'], string> = {
  java: 'Готовим Java',
  neoforge: 'Устанавливаем NeoForge',
  libraries: 'Скачиваем библиотеки',
  assets: 'Скачиваем ресурсы игры',
}

const servers: Server[] = [
  {
    id: 'aoc',
    kicker: 'Основная сборка',
    name: 'Aeronautics',
    subtitle: 'Строй корабли. Поднимай города в небо.',
    version: '1.21.1 · NeoForge 21.1.248',
    profileId: 'aeronautics',
  },
]

function isTauri() {
  return '__TAURI_INTERNALS__' in window
}

function errorMessage(error: unknown, fallback: string) {
  if (typeof error === 'string' && error.trim()) return error
  if (error instanceof Error && error.message) return error.message
  return fallback
}

function App() {
  const [selected, setSelected] = useState(servers[0])
  const [progress, setProgress] = useState<number | null>(null)
  const [ready, setReady] = useState(true)
  const [settingsOpen, setSettingsOpen] = useState(false)
  const [ram, setRam] = useState(6)
  const [nickname, setNickname] = useState('Emil')
  const [accountMode, setAccountMode] = useState<'microsoft' | 'offline'>('offline')
  const [nativeHost, setNativeHost] = useState<NativeHost | null>(null)
  const [java, setJava] = useState<JavaInstallation | null | undefined>(undefined)
  const [profile, setProfile] = useState<ProfileInspection | null>(null)
  const [syncing, setSyncing] = useState(false)
  const [syncError, setSyncError] = useState<string | null>(null)

  // undefined = still checking for a saved session; null = signed out.
  const [account, setAccount] = useState<ShaCraftAccount | null | undefined>(undefined)
  const [accountUsername, setAccountUsername] = useState('')
  const [accountPassword, setAccountPassword] = useState('')
  const [registering, setRegistering] = useState(false)
  const [recoveryCodes, setRecoveryCodes] = useState<string[]>([])
  const [linkNickname, setLinkNickname] = useState('')
  const [linkMessage, setLinkMessage] = useState<string | null>(null)
  const [loginError, setLoginError] = useState<string | null>(null)
  const [loggingIn, setLoggingIn] = useState(false)
  const [installing, setInstalling] = useState(false)
  const [gameRunning, setGameRunning] = useState(false)
  const [installProgress, setInstallProgress] = useState<InstallProgressPayload | null>(null)
  const [launchError, setLaunchError] = useState<string | null>(null)
  const [serverStatus, setServerStatus] = useState<ServerStatus | null>(null)
  const linkedNickname = account?.links.find((link) => link.server_id === 'aoc')?.mc_username

  useEffect(() => {
    if (progress === null) return
    if (progress >= 100) {
      const done = window.setTimeout(() => {
        setProgress(null)
        setReady(true)
      }, 650)
      return () => window.clearTimeout(done)
    }
    const timer = window.setTimeout(() => setProgress(Math.min(100, progress + 2)), 55)
    return () => window.clearTimeout(timer)
  }, [progress])

  useEffect(() => {
    if (!isTauri()) return
    invoke<NativeHost>('native_host').then(setNativeHost).catch(() => setNativeHost(null))
    invoke<NativeSettings>('load_settings')
      .then((settings) => {
        setRam(settings.memoryMb / 1024)
        setNickname(settings.nickname)
        setAccountMode(settings.accountMode)
      })
      .catch(() => undefined)
    invoke<JavaInstallation | null>('detect_java')
      .then(setJava)
      .catch(() => setJava(null))
    invoke<ProfileInspection>('inspect_remote_profile', { profileId: 'aeronautics' })
      .then((inspection) => {
        setProfile(inspection)
        setReady(inspection.upToDate)
      })
      .catch(() => undefined)
    invoke<ShaCraftAccount | null>('get_shacraft_account')
      .then(setAccount)
      .catch(() => setAccount(null))
  }, [])

  useEffect(() => {
    if (!isTauri()) return
    let disposed = false
    const refresh = () => {
      invoke<ServerStatus>('get_server_status', { profileId: selected.profileId })
        .then((status) => {
          if (!disposed) setServerStatus(status)
        })
        .catch(() => {
          if (!disposed) setServerStatus({ online: null, max: null, reachable: false })
        })
    }
    refresh()
    const timer = window.setInterval(refresh, 30_000)
    return () => {
      disposed = true
      window.clearInterval(timer)
    }
  }, [selected.profileId])

  useEffect(() => {
    if (!isTauri()) return
    const unlisten = [
      listen<InstallProgressPayload>('game-install-progress', (event) => setInstallProgress(event.payload)),
      listen<GameExitedPayload>('game-exited', (event) => {
        setInstalling(false)
        setGameRunning(false)
        setInstallProgress(null)
        if (event.payload.exitCode !== 0) {
          const suffix = event.payload.exitCode === null ? '' : ` (код ${event.payload.exitCode})`
          setLaunchError(`Игра завершилась с ошибкой${suffix}. Подробности сохранены в журнале лаунчера.`)
        }
      }),
    ]
    return () => {
      unlisten.forEach((promise) => promise.then((off) => off()))
    }
  }, [])

  const saveSettings = (memoryGb = ram, nick = nickname, mode = accountMode) => {
    if (isTauri()) {
      invoke<NativeSettings>('save_settings', { settings: { memoryMb: memoryGb * 1024, nickname: nick, accountMode: mode } }).catch(() => undefined)
    }
  }

  const updateRam = (memoryGb: number) => {
    setRam(memoryGb)
    saveSettings(memoryGb)
  }

  const repair = async () => {
    if (isTauri()) {
      setSyncError(null)
      setSyncing(true)
      setReady(false)
      try {
        const result = await invoke<SyncResult>('sync_remote_profile', { profileId: selected.profileId })
        setProfile({ managedFiles: result.downloadedFiles + result.reusedFiles, missingFiles: 0, mismatchedFiles: 0, upToDate: true })
        setReady(true)
      } catch (error) {
        setSyncError(errorMessage(error, 'Не удалось синхронизировать сборку'))
      } finally {
        setSyncing(false)
      }
      return
    }
    setReady(false)
    setProgress(0)
  }

  const startLogin = async () => {
    if (!isTauri()) return
    setLoginError(null)
    if (!/^[A-Za-z0-9_]{3,32}$/.test(accountUsername) || accountPassword.length < 8) {
      setLoginError('Логин: 3–32 символа; пароль: минимум 8 символов')
      return
    }
    setLoggingIn(true)
    try {
      const result = await invoke<ShaCraftLoginResult>('shacraft_authenticate', {
        username: accountUsername,
        password: accountPassword,
        register: registering,
      })
      setAccount(result.account)
      setAccountPassword('')
      setRecoveryCodes(result.recoveryCodes)
      setLoginError(null)
    } catch (error) {
      setLoginError(errorMessage(error, registering ? 'Не удалось зарегистрироваться' : 'Не удалось войти'))
    } finally {
      setLoggingIn(false)
    }
  }

  const logout = async () => {
    if (!isTauri()) return
    await invoke('shacraft_logout').catch(() => undefined)
    setAccount(null)
  }

  const startNicknameLink = async () => {
    if (!/^[A-Za-z0-9_]{3,16}$/.test(linkNickname)) {
      setLinkMessage('Ник: 3–16 латинских букв, цифр или _')
      return
    }
    setLinkMessage('Создаём проверку…')
    try {
      const started = await invoke<{ challenge_id: number; registered_on_server: boolean }>('shacraft_start_link', { nickname: linkNickname })
      setLinkMessage(started.registered_on_server
        ? 'Зайдите на Aeronautics с этим ником и выполните /login.'
        : 'Зайдите на Aeronautics с этим ником и выполните /register.')
      const timer = window.setInterval(async () => {
        try {
          const result = await invoke<{ status: string; detail?: string }>('shacraft_link_status', { challengeId: started.challenge_id })
          if (result.status === 'verified') {
            window.clearInterval(timer)
            const refreshed = await invoke<ShaCraftAccount>('get_shacraft_account')
            setAccount(refreshed)
            setLinkMessage('Ник подтверждён.')
          } else if (result.status === 'expired' || result.status === 'conflict') {
            window.clearInterval(timer)
            setLinkMessage(result.detail ?? 'Проверка завершилась. Попробуйте ещё раз.')
          }
        } catch (error) {
          window.clearInterval(timer)
          setLinkMessage(errorMessage(error, 'Не удалось проверить ник'))
        }
      }, 3000)
    } catch (error) {
      setLinkMessage(errorMessage(error, 'Не удалось начать привязку'))
    }
  }

  const playOrLogin = async () => {
    if (!isTauri()) return
    if (!account) {
      setSettingsOpen(true)
      setLaunchError('Войдите в аккаунт ShaCraft, чтобы играть.')
      return
    }
    if (!linkedNickname) {
      setSettingsOpen(true)
      setLaunchError('Привяжите игровой ник к Aeronautics, чтобы играть.')
      return
    }
    setLaunchError(null)
    setSyncError(null)
    setSyncing(true)
    setInstallProgress(null)
    try {
      // A launch must always reconcile the signed ShaCraft profile first.
      // Installing Minecraft/NeoForge alone produces a valid but unmodded
      // game, so profile sync is deliberately part of the Play path.
      const syncResult = await invoke<SyncResult>('sync_remote_profile', { profileId: selected.profileId })
      setProfile({ managedFiles: syncResult.downloadedFiles + syncResult.reusedFiles, missingFiles: 0, mismatchedFiles: 0, upToDate: true })
      setReady(true)
      setSyncing(false)
      setInstalling(true)
      await invoke('ensure_game_installed', { profileId: selected.profileId })
      setInstallProgress(null)
      setInstalling(false)
      setGameRunning(true)
      await invoke('launch_game', { profileId: selected.profileId })
    } catch (error) {
      setLaunchError(errorMessage(error, 'Не удалось запустить игру'))
      setSyncing(false)
      setInstalling(false)
      setGameRunning(false)
    }
  }

  const playLabel = () => {
    if (account === undefined) return 'Загрузка…'
    if (account === null) return 'Войти в ShaCraft'
    if (!linkedNickname) return 'Привязать ник'
    if (gameRunning) return 'Игра запущена'
    if (installing) return installProgress ? `${INSTALL_STAGE_LABEL[installProgress.stage]}…` : 'Подготовка…'
    if (syncing || progress !== null) return 'Обновление'
    return ready ? 'Играть' : 'Проверить'
  }

  const installPercent = installProgress && installProgress.totalBytes > 0 ? Math.min(100, Math.round((installProgress.currentBytes / installProgress.totalBytes) * 100)) : null
  const onlineLabel = serverStatus?.reachable && serverStatus.online !== null && serverStatus.max !== null
    ? `${serverStatus.online} / ${serverStatus.max}`
    : serverStatus === null ? 'Проверяем…' : 'Нет связи'

  const minimizeWindow = () => { if (isTauri()) void getCurrentWindow().minimize() }
  const toggleMaximizeWindow = () => { if (isTauri()) void getCurrentWindow().toggleMaximize() }
  const closeWindow = () => { if (isTauri()) void getCurrentWindow().close() }

  return (
    <div className="app-shell">
      <header className="titlebar" data-tauri-drag-region>
        <div className="brand" data-tauri-drag-region>
          <img src={logo} alt="" data-tauri-drag-region />
          <span data-tauri-drag-region>ShaCraft</span>
        </div>
        <div className="titlebar-drag" data-tauri-drag-region>{nativeHost ? `Лаунчер · ${nativeHost.platform}` : 'Лаунчер'}</div>
        <div className="window-actions" aria-label="Управление окном">
          <button aria-label="Свернуть" onClick={minimizeWindow}><Minus size={15} /></button>
          <button aria-label="Развернуть" onClick={toggleMaximizeWindow}><Square size={12} /></button>
          <button className="close" aria-label="Закрыть" onClick={closeWindow}><X size={15} /></button>
        </div>
      </header>

      <div className="workspace">
        <nav className="rail" aria-label="Настройки лаунчера">
          <button className="rail-button active" aria-label="Настройки" onClick={() => setSettingsOpen(true)}>
            <Settings />
          </button>
        </nav>

        <aside className="library-panel">
          <div className="library-heading">
            <p>Сборки</p>
            <span>1 доступна</span>
          </div>
          <div className="server-list">
            {servers.map((server) => (
              <button
                key={server.id}
                className={`server-row ${selected.id === server.id ? 'selected' : ''}`}
                onClick={() => {
                  setSelected(server)
                  setReady(profile?.upToDate ?? false)
                  setProgress(null)
                }}
              >
                <span className={`server-glyph ${server.id}`} aria-hidden="true">
                  {server.id === 'aoc' ? 'A' : 'C'}
                </span>
                <span className="server-copy">
                  <strong>{server.name}</strong>
                  <small>{profile?.upToDate ? 'Файлы проверены' : 'Требуется проверка'}</small>
                </span>
                <ChevronRight size={16} />
              </button>
            ))}
          </div>

          <button className="account-chip" onClick={() => setSettingsOpen(true)}>
            <span className="avatar">{linkedNickname ? linkedNickname.slice(0, 2).toUpperCase() : (account ? account.username.slice(0, 2).toUpperCase() : '?')}</span>
            <span>
              <strong>{account === undefined ? 'Проверяем…' : account === null ? 'Не авторизован' : (linkedNickname ?? account.username)}</strong>
              <small>{account ? `ShaCraft · ${account.username}` : 'Войдите, чтобы играть'}</small>
            </span>
            <ChevronRight size={16} />
          </button>
        </aside>

        <main className={`stage stage-${selected.id}`}>
          <div className="stage-top">
            <div className={`live-pill ${serverStatus === null || serverStatus.reachable ? '' : 'offline'}`}>
              <span /> {serverStatus === null ? 'Проверяем сервер' : serverStatus.reachable ? 'Сервер доступен' : 'Сервер недоступен'}
            </div>
            <div className="players"><Users size={16} /> {onlineLabel}</div>
          </div>

          <section className="hero-copy">
            <p>{selected.kicker}</p>
            <h1>{selected.name}</h1>
            <h2>{selected.subtitle}</h2>
            <dl className="hero-meta">
              <div><dt>Загрузчик</dt><dd>NeoForge 21.1.248</dd></div>
              <div><dt>Java</dt><dd>Версия 21</dd></div>
            </dl>
          </section>

          <section className="play-dock">
            <div className="build-state">
              {gameRunning ? (
                <>
                  <span className="state-icon"><Play size={19} /></span>
                  <span><strong>Игра запущена</strong><small>Лаунчер готов к работе после выхода</small></span>
                </>
              ) : installing ? (
                <>
                  <span className="state-icon downloading"><Download size={19} /></span>
                  <span>
                    <strong>{installProgress ? INSTALL_STAGE_LABEL[installProgress.stage] : 'Готовим установку'}</strong>
                    <small>{installPercent !== null ? `${installPercent}%` : 'Проверяем файлы…'}</small>
                  </span>
                </>
              ) : syncing ? (
                <>
                  <span className="state-icon downloading"><Download size={19} /></span>
                  <span><strong>Синхронизируем сборку</strong><small>Скачиваем и проверяем файлы</small></span>
                </>
              ) : progress !== null ? (
                <>
                  <span className="state-icon downloading"><Download size={19} /></span>
                  <span>
                    <strong>Проверяем сборку</strong>
                    <small>Файлы и обновления · {progress}%</small>
                  </span>
                </>
              ) : (
                <>
                  <span className="state-icon"><ShieldCheck size={19} /></span>
                  <span>
                    <strong>{account === null ? 'Нужен вход ShaCraft' : !linkedNickname ? 'Нужно привязать ник' : ready ? 'Файлы сборки готовы' : 'Требуется проверка'}</strong>
                    <small>{launchError || syncError || loginError || (profile ? `${profile.managedFiles} файлов сборки` : 'Проверяем локальные файлы')}</small>
                  </span>
                </>
              )}
              {progress !== null && <div className="progress-track"><i style={{ width: `${progress}%` }} /></div>}
              {installPercent !== null && <div className="progress-track"><i style={{ width: `${installPercent}%` }} /></div>}
            </div>

            <div className="build-facts">
              <span><Globe2 size={15} /> {selected.version}</span>
              <span><Gauge size={15} /> {ram} ГБ памяти</span>
            </div>

            <button className="repair-button" onClick={repair} disabled={progress !== null || syncing || installing || gameRunning} aria-label="Проверить файлы">
              <RotateCcw size={19} />
            </button>
            <button
              className="play-button"
              disabled={progress !== null || syncing || installing || gameRunning || account === undefined || loggingIn}
              onClick={playOrLogin}
            >
              <Play size={21} fill="currentColor" />
              <span>{playLabel()}</span>
            </button>
          </section>
        </main>
      </div>

      <div className={`drawer-backdrop ${recoveryCodes.length ? 'visible' : ''}`} />
      {recoveryCodes.length > 0 && (
        <div className="login-modal" role="dialog" aria-modal="true">
          <h2>Коды восстановления</h2>
          <p>Сохраните их сейчас. Каждый код можно использовать один раз для восстановления пароля.</p>
          <div className="login-code" style={{ whiteSpace: 'pre-line', fontSize: '15px' }}>{recoveryCodes.join('\n')}</div>
          <button className="setting-row" onClick={() => setRecoveryCodes([])}><span>Я сохранил коды</span></button>
        </div>
      )}

      <div className={`drawer-backdrop ${settingsOpen ? 'visible' : ''}`} onClick={() => setSettingsOpen(false)} />
      <aside className={`settings-drawer ${settingsOpen ? 'open' : ''}`} aria-hidden={!settingsOpen}>
        <div className="drawer-title">
          <div><p>Настройки</p><h2>Игра</h2></div>
          <button onClick={() => setSettingsOpen(false)} aria-label="Закрыть настройки"><X /></button>
        </div>
        <label className="range-setting">
          <span><strong>Оперативная память</strong><b>{ram} ГБ</b></span>
          <input type="range" min="3" max="12" value={ram} onChange={(e) => updateRam(Number(e.target.value))} />
          <small>Для Aeronautics рекомендуется 6 ГБ</small>
        </label>
        <div className="setting-row static">
          <span><Users />Аккаунт</span>
          <small>{account ? account.username : 'Не авторизован'}</small>
        </div>
        {!account && (
          <>
            <label className="text-setting">
              <span><strong>Логин ShaCraft</strong><small>3–32 символа</small></span>
              <input value={accountUsername} maxLength={32} autoComplete="username" onChange={(event) => setAccountUsername(event.target.value)} placeholder="Логин" />
            </label>
            <label className="text-setting">
              <span><strong>Пароль</strong><small>Минимум 8 символов</small></span>
              <input type="password" value={accountPassword} maxLength={128} autoComplete={registering ? 'new-password' : 'current-password'} onChange={(event) => setAccountPassword(event.target.value)} placeholder="Пароль" />
            </label>
            {loginError && <div className="drawer-note">{loginError}</div>}
            <button className="setting-row" onClick={startLogin} disabled={loggingIn}>
              <span>{loggingIn ? 'Подождите…' : registering ? 'Создать аккаунт' : 'Войти'}</span>
            </button>
            <button className="setting-row" onClick={() => { setRegistering(!registering); setLoginError(null) }}>
              <span>{registering ? 'Уже есть аккаунт' : 'Нет аккаунта — регистрация'}</span>
            </button>
          </>
        )}
        {account && !linkedNickname && (
          <>
            <label className="text-setting">
              <span><strong>Игровой ник</strong><small>Aeronautics</small></span>
              <input value={linkNickname} maxLength={16} onChange={(event) => setLinkNickname(event.target.value)} placeholder="Player" />
              <small>Ник нельзя будет подменить локальной настройкой</small>
            </label>
            <button className="setting-row" onClick={startNicknameLink}><span>Привязать ник</span></button>
            {linkMessage && <div className="drawer-note">{linkMessage}</div>}
          </>
        )}
        {account && linkedNickname && (
          <div className="setting-row static">
            <span>Игровой ник</span><small>{linkedNickname}</small>
          </div>
        )}
        {account && (
          <button className="setting-row" onClick={logout}>
            <span><LogOut />Выйти из ShaCraft</span>
          </button>
        )}
        <div className="setting-row static">
          <span><FolderOpen />Папка игры</span>
          <small>{nativeHost ? 'В каталоге лаунчера' : 'Определяется…'}</small>
        </div>
        <div className="setting-row static">
          <span><Wrench />Java</span>
          <small>
            {java === undefined
              ? 'Проверяем…'
              : java && java.major >= 21
                ? `Java ${java.major} найдена`
                : java
                  ? `Нужна Java 21 · найдена ${java.major}`
                  : 'Лаунчер установит Java 21 автоматически'}
          </small>
        </div>
        <div className="drawer-note">
          {nativeHost ? `Данные лаунчера: ${nativeHost.dataDir}` : 'Java 21 будет управляться лаунчером автоматически.'}
        </div>
      </aside>
    </div>
  )
}

createRoot(document.getElementById('root')!).render(
  <React.StrictMode><App /></React.StrictMode>,
)

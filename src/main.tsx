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

type MinecraftProfile = {
  id: string
  name: string
}

type DeviceCodePayload = {
  verificationUri: string
  userCode: string
  expiresInSeconds: number
}

type LoginResultPayload = {
  ok: boolean
  profile?: MinecraftProfile
  error?: string
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
  const [account, setAccount] = useState<MinecraftProfile | null | undefined>(undefined)
  const [loginCode, setLoginCode] = useState<DeviceCodePayload | null>(null)
  const [loginError, setLoginError] = useState<string | null>(null)
  const [loggingIn, setLoggingIn] = useState(false)
  const [installing, setInstalling] = useState(false)
  const [gameRunning, setGameRunning] = useState(false)
  const [installProgress, setInstallProgress] = useState<InstallProgressPayload | null>(null)
  const [launchError, setLaunchError] = useState<string | null>(null)
  const [serverStatus, setServerStatus] = useState<ServerStatus | null>(null)
  const [microsoftLoginAvailable, setMicrosoftLoginAvailable] = useState(false)

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
    invoke<boolean>('microsoft_login_available')
      .then(setMicrosoftLoginAvailable)
      .catch(() => setMicrosoftLoginAvailable(false))
    invoke<ProfileInspection>('inspect_remote_profile', { profileId: 'aeronautics' })
      .then((inspection) => {
        setProfile(inspection)
        setReady(inspection.upToDate)
      })
      .catch(() => undefined)
    invoke<MinecraftProfile | null>('get_account')
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
      listen<DeviceCodePayload>('msa-login-code', (event) => setLoginCode(event.payload)),
      listen<LoginResultPayload>('msa-login-result', (event) => {
        setLoggingIn(false)
        setLoginCode(null)
        if (event.payload.ok && event.payload.profile) {
          setAccount(event.payload.profile)
          setLoginError(null)
        } else {
          setLoginError(event.payload.error ?? 'Не удалось войти через Microsoft')
        }
      }),
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

  const saveNickname = () => {
    if (/^[A-Za-z0-9_]{3,16}$/.test(nickname)) {
      saveSettings(ram, nickname)
    }
  }

  const setMode = (mode: 'microsoft' | 'offline') => {
    setAccountMode(mode)
    saveSettings(ram, nickname, mode)
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
    if (!isTauri() || !microsoftLoginAvailable) {
      setLoginError('Вход через Microsoft пока не настроен для этой версии лаунчера')
      return
    }
    setLoginError(null)
    setLoggingIn(true)
    try {
      await invoke('start_microsoft_login')
    } catch (error) {
      setLoggingIn(false)
      setLoginError(errorMessage(error, 'Не удалось начать вход через Microsoft'))
    }
  }

  const logout = async () => {
    if (!isTauri()) return
    await invoke('logout').catch(() => undefined)
    setAccount(null)
  }

  const playOrLogin = async () => {
    if (!isTauri()) return
    if (accountMode === 'microsoft' && !microsoftLoginAvailable) {
      setLaunchError('Вход Microsoft пока недоступен. Выберите Offline-аккаунт в настройках.')
      return
    }
    // In offline mode we can launch without any Microsoft session. In
    // Microsoft mode a signed-in account is still required first.
    if (accountMode === 'microsoft' && (account === null || account === undefined)) {
      await startLogin()
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
    if (accountMode === 'microsoft' && !microsoftLoginAvailable) return 'Microsoft недоступен'
    if (accountMode === 'microsoft' && account === undefined) return 'Загрузка…'
    if (accountMode === 'microsoft' && account === null) return loggingIn ? 'Ждём вход…' : 'Войти через Microsoft'
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
            <span className="avatar">{accountMode === 'offline' ? nickname.slice(0, 2).toUpperCase() : (account ? account.name.slice(0, 2).toUpperCase() : '?')}</span>
            <span>
              <strong>{accountMode === 'offline' ? nickname : (account === undefined ? 'Проверяем…' : account === null ? 'Не авторизован' : account.name)}</strong>
              <small>{accountMode === 'offline' ? 'Offline-аккаунт' : (account ? 'Microsoft-аккаунт' : 'Войдите, чтобы играть')}</small>
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
                    <strong>{accountMode === 'microsoft' && !microsoftLoginAvailable ? 'Microsoft пока недоступен' : accountMode === 'microsoft' && account === null ? 'Нужен вход' : ready ? 'Файлы сборки готовы' : 'Требуется проверка'}</strong>
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
              disabled={progress !== null || syncing || installing || gameRunning || (accountMode === 'microsoft' && (account === undefined || !microsoftLoginAvailable)) || loggingIn}
              onClick={playOrLogin}
            >
              <Play size={21} fill="currentColor" />
              <span>{playLabel()}</span>
            </button>
          </section>
        </main>
      </div>

      <div className={`drawer-backdrop ${loginCode ? 'visible' : ''}`} />
      {loginCode && (
        <div className="login-modal" role="dialog" aria-modal="true">
          <h2>Вход через Microsoft</h2>
          <p>Откройте страницу и введите код, чтобы подтвердить вход в аккаунт с лицензией Minecraft.</p>
          <div className="login-code">{loginCode.userCode}</div>
          <p className="login-url">{loginCode.verificationUri}</p>
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
          <small>{accountMode === 'offline' ? 'Offline' : (microsoftLoginAvailable ? (account ? account.name : 'Не авторизован') : 'Временно недоступен')}</small>
        </div>
        {accountMode === 'offline' && (
          <label className="text-setting">
            <span><strong>Игровой ник</strong><small>Offline-профиль</small></span>
            <input value={nickname} maxLength={16} onChange={(event) => setNickname(event.target.value)} onBlur={saveNickname} placeholder="Player" />
            <small>Латинские буквы, цифры и _ · от 3 до 16 символов</small>
          </label>
        )}
        <div className="setting-row">
          <span>Тип аккаунта</span>
          <select
            value={accountMode}
            onChange={(e) => setMode(e.target.value as 'microsoft' | 'offline')}
            style={{ background: 'transparent', border: 0, color: 'inherit', textAlign: 'right' }}
          >
            <option value="offline">Offline</option>
            <option value="microsoft" disabled={!microsoftLoginAvailable}>Microsoft (скоро)</option>
          </select>
        </div>
        {accountMode === 'microsoft' && account && (
          <button className="setting-row" onClick={logout}>
            <span><LogOut />Выйти из Microsoft</span>
          </button>
        )}
        {accountMode === 'microsoft' && !account && microsoftLoginAvailable && (
          <button className="setting-row" onClick={startLogin}>
            <span><LogOut />Войти через Microsoft</span>
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

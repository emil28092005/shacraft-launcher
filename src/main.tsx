import React, { useEffect, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { invoke } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import {
  ChevronRight,
  Download,
  FolderOpen,
  Gauge,
  Globe2,
  Library,
  LogOut,
  MessageCircle,
  Minus,
  Newspaper,
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
  players: string
  version: string
  memory: string
  installed: boolean
  profileId?: string
  disabled?: boolean
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
    players: '7 / 20',
    version: '1.21.1 · NeoForge',
    memory: '6 ГБ',
    installed: true,
    profileId: 'aeronautics',
  },
  {
    id: 'create',
    kicker: 'На техобслуживании',
    name: 'Create',
    subtitle: 'Механизмы, фабрики и большие идеи.',
    players: 'Сервер остановлен',
    version: '1.21.1 · NeoForge',
    memory: '4 ГБ',
    installed: false,
    disabled: true,
  },
]

function isTauri() {
  return '__TAURI_INTERNALS__' in window
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
  const [installProgress, setInstallProgress] = useState<InstallProgressPayload | null>(null)
  const [launchError, setLaunchError] = useState<string | null>(null)

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
    invoke<MinecraftProfile | null>('get_account')
      .then(setAccount)
      .catch(() => setAccount(null))
  }, [])

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
      listen('game-exited', () => setInstalling(false)),
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
    if (selected.disabled) return
    if (isTauri() && selected.profileId) {
      setSyncError(null)
      setSyncing(true)
      setReady(false)
      try {
        const result = await invoke<SyncResult>('sync_remote_profile', { profileId: selected.profileId })
        setProfile({ managedFiles: result.downloadedFiles + result.reusedFiles, missingFiles: 0, mismatchedFiles: 0, upToDate: true })
        setReady(true)
      } catch (error) {
        setSyncError(error instanceof Error ? error.message : 'Не удалось синхронизировать сборку')
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
    setLoggingIn(true)
    try {
      await invoke('start_microsoft_login')
    } catch (error) {
      setLoggingIn(false)
      setLoginError(error instanceof Error ? error.message : 'Не удалось начать вход через Microsoft')
    }
  }

  const logout = async () => {
    if (!isTauri()) return
    await invoke('logout').catch(() => undefined)
    setAccount(null)
  }

  const playOrLogin = async () => {
    if (selected.disabled || !isTauri() || !selected.profileId) return
    // In offline mode we can launch without any Microsoft session. In
    // Microsoft mode a signed-in account is still required first.
    if (accountMode === 'microsoft' && (account === null || account === undefined)) {
      await startLogin()
      return
    }
    setLaunchError(null)
    setInstalling(true)
    setInstallProgress(null)
    try {
      await invoke('ensure_game_installed', { profileId: selected.profileId })
      await invoke('launch_game', { profileId: selected.profileId })
    } catch (error) {
      setLaunchError(error instanceof Error ? error.message : 'Не удалось запустить игру')
      setInstalling(false)
    }
  }

  const playLabel = () => {
    if (selected.disabled) return 'Недоступно'
    if (accountMode === 'microsoft' && account === undefined) return 'Загрузка…'
    if (accountMode === 'microsoft' && account === null) return loggingIn ? 'Ждём вход…' : 'Войти через Microsoft'
    if (installing) return installProgress ? `${INSTALL_STAGE_LABEL[installProgress.stage]}…` : 'Подготовка…'
    if (syncing || progress !== null) return 'Обновление'
    return ready ? 'Играть' : 'Проверить'
  }

  const installPercent = installProgress && installProgress.totalBytes > 0 ? Math.min(100, Math.round((installProgress.currentBytes / installProgress.totalBytes) * 100)) : null

  return (
    <div className="app-shell">
      <header className="titlebar">
        <div className="brand">
          <img src={logo} alt="" />
          <span>ShaCraft</span>
        </div>
        <div className="titlebar-drag">{nativeHost ? `Лаунчер · ${nativeHost.platform}` : 'Лаунчер'}</div>
        <div className="window-actions" aria-label="Управление окном">
          <button aria-label="Свернуть"><Minus size={15} /></button>
          <button aria-label="Развернуть"><Square size={12} /></button>
          <button className="close" aria-label="Закрыть"><X size={15} /></button>
        </div>
      </header>

      <div className="workspace">
        <nav className="rail" aria-label="Основное меню">
          <div className="rail-main">
            <button className="rail-button active" aria-label="Сборки"><Library /></button>
            <button className="rail-button" aria-label="Новости"><Newspaper /></button>
            <button className="rail-button" aria-label="Сообщество"><MessageCircle /></button>
          </div>
          <button className="rail-button" aria-label="Настройки" onClick={() => setSettingsOpen(true)}>
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
                  setReady(server.installed)
                  setProgress(null)
                }}
              >
                <span className={`server-glyph ${server.id}`} aria-hidden="true">
                  {server.id === 'aoc' ? 'A' : 'C'}
                </span>
                <span className="server-copy">
                  <strong>{server.name}</strong>
                  <small>{server.disabled ? 'На паузе' : 'Установлена'}</small>
                </span>
                <ChevronRight size={16} />
              </button>
            ))}
          </div>

          <div className="account-chip">
            <span className="avatar">{accountMode === 'offline' ? nickname.slice(0, 2).toUpperCase() : (account ? account.name.slice(0, 2).toUpperCase() : '?')}</span>
            <span>
              <strong>{accountMode === 'offline' ? nickname : (account === undefined ? 'Проверяем…' : account === null ? 'Не авторизован' : account.name)}</strong>
              <small>{accountMode === 'offline' ? 'Offline-аккаунт' : (account ? 'Microsoft-аккаунт' : 'Войдите, чтобы играть')}</small>
            </span>
            {accountMode === 'microsoft' && account ? (
              <button aria-label="Выйти из аккаунта" onClick={logout} style={{ background: 'transparent', border: 0, cursor: 'pointer', color: 'inherit' }}>
                <LogOut size={16} />
              </button>
            ) : (
              <ChevronRight size={16} />
            )}
          </div>
        </aside>

        <main className={`stage stage-${selected.id}`}>
          <div className="stage-top">
            <div className={`live-pill ${selected.disabled ? 'offline' : ''}`}>
              <span /> {selected.disabled ? 'Не в сети' : 'Сервер работает'}
            </div>
            <div className="players"><Users size={16} /> {selected.players}</div>
          </div>

          <section className="hero-copy">
            <p>{selected.id === 'aoc' ? 'All of Create / сборка 2.5' : selected.kicker}</p>
            <h1>{selected.name}</h1>
            <h2>{selected.subtitle}</h2>
            <dl className="hero-meta">
              <div><dt>Состав</dt><dd>{selected.id === 'aoc' ? '250 модов' : '41 мод'}</dd></div>
              <div><dt>Загрузчик</dt><dd>{selected.id === 'aoc' ? 'NeoForge 21.1.248' : 'NeoForge 21.1.249'}</dd></div>
              <div><dt>Java</dt><dd>Версия 21</dd></div>
            </dl>
          </section>

          <section className="play-dock">
            <div className="build-state">
              {installing ? (
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
              ) : selected.disabled ? (
                <>
                  <span className="state-icon muted"><Wrench size={19} /></span>
                  <span><strong>Техобслуживание</strong><small>Сообщим, когда сервер вернётся</small></span>
                </>
              ) : (
                <>
                  <span className="state-icon"><ShieldCheck size={19} /></span>
                  <span>
                    <strong>{accountMode === 'microsoft' && account === null ? 'Нужен вход' : ready ? 'Сборка готова' : 'Требуется проверка'}</strong>
                    <small>{launchError || syncError || loginError || (profile ? `${profile.managedFiles} файлов под контролем` : 'Проверяем локальные файлы')}</small>
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

            <button className="repair-button" onClick={repair} disabled={progress !== null || syncing || installing || selected.disabled} aria-label="Проверить файлы">
              <RotateCcw size={19} />
            </button>
            <button
              className="play-button"
              disabled={progress !== null || syncing || installing || selected.disabled || (accountMode === 'microsoft' && account === undefined) || loggingIn}
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
          <small>{accountMode === 'offline' ? 'Offline' : (account ? account.name : 'Не авторизован')}</small>
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
            <option value="microsoft">Microsoft</option>
          </select>
        </div>
        {accountMode === 'microsoft' && account && (
          <button className="setting-row" onClick={logout}>
            <span><LogOut />Выйти из Microsoft</span>
          </button>
        )}
        {accountMode === 'microsoft' && !account && (
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
        <button className="setting-row"><span><Wrench />Дополнительные параметры</span><ChevronRight /></button>
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

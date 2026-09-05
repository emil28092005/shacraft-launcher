import React, { useEffect, useState } from 'react'
import { createRoot } from 'react-dom/client'
import { invoke } from '@tauri-apps/api/core'
import {
  ChevronRight,
  Download,
  FolderOpen,
  Gauge,
  Globe2,
  Library,
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
  disabled?: boolean
}

type NativeHost = {
  platform: string
  dataDir: string
  launcherVersion: string
}

type NativeSettings = {
  memoryMb: number
}

type JavaInstallation = {
  executable: string
  major: number
  version: string
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

function App() {
  const [selected, setSelected] = useState(servers[0])
  const [progress, setProgress] = useState<number | null>(null)
  const [ready, setReady] = useState(true)
  const [settingsOpen, setSettingsOpen] = useState(false)
  const [ram, setRam] = useState(6)
  const [nativeHost, setNativeHost] = useState<NativeHost | null>(null)
  const [java, setJava] = useState<JavaInstallation | null | undefined>(undefined)

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
    if (!('__TAURI_INTERNALS__' in window)) return
    invoke<NativeHost>('native_host').then(setNativeHost).catch(() => setNativeHost(null))
    invoke<NativeSettings>('load_settings')
      .then((settings) => setRam(settings.memoryMb / 1024))
      .catch(() => undefined)
    invoke<JavaInstallation | null>('detect_java')
      .then(setJava)
      .catch(() => setJava(null))
  }, [])

  const updateRam = (memoryGb: number) => {
    setRam(memoryGb)
    if ('__TAURI_INTERNALS__' in window) {
      invoke<NativeSettings>('save_settings', { settings: { memoryMb: memoryGb * 1024 } })
        .catch(() => undefined)
    }
  }

  const repair = () => {
    if (selected.disabled) return
    setReady(false)
    setProgress(0)
  }

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
            <span className="avatar">ES</span>
            <span><strong>Émile</strong><small>offline-профиль</small></span>
            <ChevronRight size={16} />
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
              {progress !== null ? (
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
                  <span><strong>{ready ? 'Сборка готова' : 'Требуется проверка'}</strong><small>Обновлено сегодня в 21:42</small></span>
                </>
              )}
              {progress !== null && <div className="progress-track"><i style={{ width: `${progress}%` }} /></div>}
            </div>

            <div className="build-facts">
              <span><Globe2 size={15} /> {selected.version}</span>
              <span><Gauge size={15} /> {ram} ГБ памяти</span>
            </div>

            <button className="repair-button" onClick={repair} disabled={progress !== null || selected.disabled} aria-label="Проверить файлы">
              <RotateCcw size={19} />
            </button>
            <button
              className="play-button"
              disabled={progress !== null || selected.disabled}
              onClick={() => !ready && repair()}
            >
              <Play size={21} fill="currentColor" />
              <span>{selected.disabled ? 'Недоступно' : progress !== null ? 'Обновление' : ready ? 'Играть' : 'Проверить'}</span>
            </button>
          </section>
        </main>
      </div>

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
                  : 'Java не найдена'}
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

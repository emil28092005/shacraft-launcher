import { useCallback, useState } from 'react'
import { Library } from './components/Library'
import { LoginModal } from './components/LoginModal'
import { PlayDock } from './components/PlayDock'
import { ServerStage } from './components/ServerStage'
import { SettingsDrawer } from './components/SettingsDrawer'
import { Titlebar } from './components/Titlebar'
import { servers } from './data/servers'
import { useAccount } from './hooks/useAccount'
import { useLauncher } from './hooks/useLauncher'
import { useSettings } from './hooks/useSettings'
import { isNative } from './services/native'
import { installStageLabels } from './state/game'

export function App() {
  const [selected, setSelected] = useState(servers[0])
  const [settingsOpen, setSettingsOpen] = useState(false)
  const preferences = useSettings()
  const session = useAccount()
  const launcher = useLauncher()
  const closeSettings = useCallback(() => setSettingsOpen(false), [])
  const desktop = isNative()
  const profile = selected.profileId ? launcher.profiles[selected.profileId] : undefined
  const ready = profile?.inspection?.upToDate === true
  const operation = launcher.game.operation
  const busy = operation.phase !== 'idle'
  const microsoft = preferences.settings.accountMode === 'microsoft'
  const needsLogin = microsoft && session.account === null
  const accountLoading = microsoft && session.account === undefined
  const checking = desktop && (!profile || profile.status === 'checking') && !selected.disabled
  const settingsBlocked = !preferences.loaded || preferences.saving || !!preferences.error || (!microsoft && !!preferences.nicknameError)
  const disabled = !desktop || !!selected.disabled || !selected.profileId || busy || checking || settingsBlocked || accountLoading || session.busy || !launcher.eventsReady || (needsLogin && !session.eventsReady)
  const repairDisabled = !desktop || !!selected.disabled || !selected.profileId || busy || checking
  const error = launcher.game.error ?? preferences.error ?? (!microsoft ? preferences.nicknameError : null) ?? launcher.environmentError ?? (microsoft ? session.error : null) ?? profile?.error ?? null

  let label = 'Играть'
  if (selected.disabled) label = 'Недоступно'
  else if (!desktop) label = 'В приложении'
  else if (operation.phase === 'running') label = 'Игра запущена'
  else if (operation.phase === 'launching') label = 'Запускаем…'
  else if (operation.phase === 'installing') label = operation.progress ? `${installStageLabels[operation.progress.stage]}…` : 'Подготовка…'
  else if (operation.phase === 'syncing') label = 'Обновление'
  else if (preferences.saving) label = 'Сохраняем…'
  else if (accountLoading || !preferences.loaded) label = 'Загрузка…'
  else if (needsLogin) label = session.busy ? 'Ждём вход…' : 'Войти через Microsoft'
  else if (checking) label = 'Проверяем…'
  else if (!ready) label = 'Проверить'

  const primary = () => {
    if (disabled || !selected.profileId) return
    if (needsLogin) void session.login()
    else if (!ready) void launcher.repair(selected.profileId)
    else void launcher.launch(selected.profileId)
  }

  return (
    <div className="app-shell">
      <Titlebar host={launcher.host} />
      <div className="workspace">
        <Library selected={selected} profiles={launcher.profiles} settings={preferences.settings}
          account={session.account} native={desktop} locked={busy || session.busy}
          onSelect={setSelected} onSettings={() => setSettingsOpen(true)} onLogout={session.logout} />
        <ServerStage server={selected}>
          <PlayDock server={selected} operation={operation} profile={profile}
            memoryGb={preferences.settings.memoryMb / 1024} native={desktop} needsLogin={needsLogin}
            error={error} label={label} primaryDisabled={disabled} repairDisabled={repairDisabled}
            onPrimary={primary} onRepair={() => { if (!repairDisabled && selected.profileId) void launcher.repair(selected.profileId) }} />
        </ServerStage>
      </div>
      <SettingsDrawer open={settingsOpen && !session.code} locked={busy || session.busy} preferences={preferences}
        session={session} host={launcher.host} java={launcher.java} onClose={closeSettings} />
      <LoginModal code={session.code} />
    </div>
  )
}

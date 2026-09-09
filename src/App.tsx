import { useCallback, useState } from 'react'
import { LegacyModsDialog } from './components/LegacyModsDialog'
import { Library } from './components/Library'
import { RecoveryCodesModal } from './components/RecoveryCodesModal'
import { PlayDock } from './components/PlayDock'
import { ServerStage } from './components/ServerStage'
import { SettingsDrawer } from './components/SettingsDrawer'
import { Titlebar } from './components/Titlebar'
import { servers } from './data/servers'
import { useAccount } from './hooks/useAccount'
import { useLauncher } from './hooks/useLauncher'
import { useServerStatus } from './hooks/useServerStatus'
import { useSettings } from './hooks/useSettings'
import { isNative } from './services/native'
import { launchAccess } from './state/account'
import { installStageLabels } from './state/game'

export function App() {
  const [selected, setSelected] = useState(servers[0])
  const [settingsOpen, setSettingsOpen] = useState(false)
  const [legacyOpen, setLegacyOpen] = useState(false)
  const [windowError, setWindowError] = useState<string | null>(null)
  const preferences = useSettings()
  const session = useAccount()
  const launcher = useLauncher()
  const serverStatus = useServerStatus(selected.profileId)
  const closeSettings = useCallback(() => setSettingsOpen(false), [])
  const desktop = isNative()
  const profile = launcher.profiles[selected.profileId]
  const metadata = launcher.metadata[selected.profileId]
  const displayed = { ...selected, version: metadata ? `Minecraft ${metadata.minecraftVersion}` : 'Версия уточняется',
    loader: metadata ? `${metadata.loaderKind} ${metadata.loaderVersion}` : 'По подписанной сборке' }
  const ready = profile?.inspection?.upToDate === true
  const operation = launcher.game.operation
  const busy = operation.phase !== 'idle'
  const access = launchAccess(session.account)
  const checking = desktop && (!profile || profile.status === 'checking')
  const settingsBlocked = !preferences.loaded || preferences.saving || !!preferences.error
  const disabled = !desktop || busy || session.busy || access === 'loading' ||
    (access === 'ready' && (checking || settingsBlocked || !launcher.eventsReady))
  const repairDisabled = !desktop || busy || checking
  const error = launcher.game.error ?? preferences.error ?? windowError ?? launcher.environmentError ?? session.error ?? profile?.error ?? null

  let label = 'Играть'
  if (!desktop) label = 'В приложении'
  else if (operation.phase === 'running') label = 'Игра запущена'
  else if (operation.phase === 'launching') label = 'Запускаем…'
  else if (operation.phase === 'installing') label = operation.progress ? `${installStageLabels[operation.progress.stage]}…` : 'Подготовка…'
  else if (operation.phase === 'syncing') label = 'Обновление'
  else if (access === 'loading') label = 'Загрузка…'
  else if (access === 'login') label = 'Войти в ShaCraft'
  else if (access === 'link') label = 'Привязать ник'
  else if (preferences.saving) label = 'Сохраняем…'
  else if (!preferences.loaded) label = 'Загрузка…'
  else if (checking) label = 'Проверяем…'
  else if (!ready) label = 'Проверить'

  const onboard = async (nickname: string) => {
    if (busy || settingsBlocked || !launcher.eventsReady) return
    const result = await launcher.onboard(selected.profileId, nickname)
    if (result?.onboarding) session.acceptChallenge(result.onboarding)
  }
  const primary = () => {
    if (disabled) return
    if (access !== 'ready') setSettingsOpen(true)
    else void launcher.launch(selected.profileId)
  }

  return (
    <div className="app-shell">
      <Titlebar host={launcher.host} onError={setWindowError} />
      <div className="workspace" inert={session.recoveryCodes.length > 0 || legacyOpen}>
        <Library selected={selected} profiles={launcher.profiles} account={session.account}
          native={desktop} locked={busy || session.busy} onSelect={setSelected} onSettings={() => setSettingsOpen(true)} />
        <ServerStage server={displayed} status={serverStatus} javaMajor={metadata?.javaMajor}>
          <PlayDock server={displayed} operation={operation} profile={profile}
            memoryGb={preferences.settings.memoryMb / 1024} native={desktop}
            needsLogin={access === 'login'} needsLink={access === 'link'}
            error={error} label={label} primaryDisabled={disabled} repairDisabled={repairDisabled}
            onLegacy={() => setLegacyOpen(true)} onPrimary={primary} onRepair={() => { if (!repairDisabled) void launcher.repair(selected.profileId) }} />
        </ServerStage>
      </div>
      <SettingsDrawer open={settingsOpen && !session.recoveryCodes.length} locked={busy} preferences={preferences}
        session={session} host={launcher.host} java={launcher.java} requiredJava={metadata?.javaMajor} onOnboard={onboard} onClose={closeSettings} />
      {legacyOpen && <LegacyModsDialog profileId={selected.profileId} onClose={() => setLegacyOpen(false)} onChanged={() => { void launcher.refreshProfile(selected.profileId) }} />}
      <RecoveryCodesModal codes={session.recoveryCodes} onAcknowledge={session.acknowledgeRecoveryCodes} />
    </div>
  )
}

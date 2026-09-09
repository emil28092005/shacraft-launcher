import { invoke, isTauri } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { createSubscription, singleFlight } from './async'
import type {
  DeviceCodePayload, GameExitedPayload, InstallProgressPayload,
  JavaInstallation, LauncherSettings, LoginResultPayload, MinecraftProfile,
  NativeHost, ProfileInspection, SyncResult,
} from '../types/launcher'

export const isNative = () => typeof window !== 'undefined' && isTauri()
const restoreAccount = singleFlight(() => invoke<MinecraftProfile | null>('get_account'))

// Keep the IPC contract in one place. UI components never invoke native
// commands directly and cannot pass arbitrary URLs or filesystem paths.
export const native = {
  host: () => invoke<NativeHost>('native_host'),
  loadSettings: () => invoke<LauncherSettings>('load_settings'),
  saveSettings: (settings: LauncherSettings) => invoke<LauncherSettings>('save_settings', { settings }),
  detectJava: () => invoke<JavaInstallation | null>('detect_java'),
  inspectProfile: (profileId: string) => invoke<ProfileInspection>('inspect_remote_profile', { profileId }),
  syncProfile: (profileId: string) => invoke<SyncResult>('sync_remote_profile', { profileId }),
  getAccount: restoreAccount,
  startLogin: () => invoke<void>('start_microsoft_login'),
  logout: () => invoke<void>('logout'),
  installGame: (profileId: string) => invoke<void>('ensure_game_installed', { profileId }),
  launchGame: (profileId: string) => invoke<void>('launch_game', { profileId }),
}

export function watchAccount(handlers: {
  code: (payload: DeviceCodePayload) => void
  result: (payload: LoginResultPayload) => void
}) {
  return createSubscription([
    listen<DeviceCodePayload>('msa-login-code', ({ payload }) => handlers.code(payload)),
    listen<LoginResultPayload>('msa-login-result', ({ payload }) => handlers.result(payload)),
  ])
}

export function watchGame(handlers: {
  progress: (payload: InstallProgressPayload) => void
  exited: (payload: GameExitedPayload) => void
}) {
  return createSubscription([
    listen<InstallProgressPayload>('game-install-progress', ({ payload }) => handlers.progress(payload)),
    listen<GameExitedPayload>('game-exited', ({ payload }) => handlers.exited(payload)),
  ])
}

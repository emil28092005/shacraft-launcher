import { invoke, isTauri } from '@tauri-apps/api/core'
import { listen } from '@tauri-apps/api/event'
import { getCurrentWindow } from '@tauri-apps/api/window'
import { createSerialQueue, createSubscription, singleFlight } from './async'
import type {
  GameExitedPayload, InstallProgressPayload, JavaInstallation, LauncherSettings,
  LinkChallenge, LinkStatus, NativeHost, ProfileInspection, ServerStatus,
  ShaCraftAccount, ShaCraftLoginResult, SyncResult,
} from '../types/launcher'

export const isNative = () => typeof window !== 'undefined' && isTauri()
const accountRequests = createSerialQueue()
const restoreAccount = singleFlight(() => accountRequests.enqueue(() => invoke<ShaCraftAccount | null>('get_shacraft_account')))

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
  authenticate: (username: string, password: string, register: boolean) =>
    accountRequests.enqueue(() => invoke<ShaCraftLoginResult>('shacraft_authenticate', { username, password, register })),
  logout: () => accountRequests.enqueue(() => invoke<void>('shacraft_logout')),
  startLink: (nickname: string) => accountRequests.enqueue(() => invoke<LinkChallenge>('shacraft_start_link', { nickname })),
  claimNickname: (nickname: string) => accountRequests.enqueue(() => invoke<ShaCraftAccount>('shacraft_claim_nickname', { nickname })),
  linkStatus: (challengeId: number) => accountRequests.enqueue(() => invoke<LinkStatus>('shacraft_link_status', { challengeId })),
  serverStatus: (profileId: string) => invoke<ServerStatus>('get_server_status', { profileId }),
  installGame: (profileId: string) => invoke<void>('ensure_game_installed', { profileId }),
  launchGame: (profileId: string) => invoke<void>('launch_game', { profileId }),
}

export const windowControls = {
  minimize: () => getCurrentWindow().minimize(),
  toggleMaximize: () => getCurrentWindow().toggleMaximize(),
  close: () => getCurrentWindow().close(),
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

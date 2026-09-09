export type AccountMode = 'microsoft' | 'offline'

export interface Server {
  id: string
  kicker: string
  name: string
  subtitle: string
  version: string
  loader: string
  profileId: string
}

export interface NativeHost {
  platform: string
  dataDir: string
  launcherVersion: string
}

export interface LauncherSettings {
  memoryMb: number
  nickname: string
  accountMode: AccountMode
}

export interface JavaInstallation {
  executable: string
  major: number
  version: string
}

export interface ProfileInspection {
  root: string
  managedFiles: number
  missingFiles: number
  mismatchedFiles: number
  upToDate: boolean
}

export interface SyncResult {
  root: string
  downloadedFiles: number
  reusedFiles: number
  downloadedBytes: number
}

export interface ShaCraftAccount {
  username: string
  links: { server_id: string; mc_username: string }[]
}

export interface ShaCraftLoginResult {
  account: ShaCraftAccount
  recoveryCodes: string[]
}

export interface LinkChallenge {
  challenge_id: number
  expires_in_seconds: number
  registered_on_server: boolean
}

export interface LinkStatus {
  status: string
  detail?: string | null
}

export interface ServerStatus {
  online: number | null
  max: number | null
  reachable: boolean
}

export interface InstallProgressPayload {
  stage: 'java' | 'neoforge' | 'libraries' | 'assets'
  currentBytes: number
  totalBytes: number
}

export interface GameExitedPayload {
  profileId: string
  exitCode: number | null
}

export interface LauncherUpdateStatus {
  currentVersion: string
  supported: boolean
  reason?: string | null
  version?: string | null
  notes?: string | null
  stage?: 'idle' | 'checking' | 'available' | 'downloading' | 'installing' | 'ready'
}

export interface LauncherUpdateProgress {
  stage: 'downloading' | 'installing' | 'ready'
  downloadedBytes: number
  totalBytes?: number | null
}

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
  staleFiles?: number
  conflicts?: string[]
  pendingUpdate?: boolean
  legacyFiles?: number
}

export interface ProfileMetadata {
  snapshot: string
  minecraftVersion: string
  loaderKind: string
  loaderVersion: string
  javaMajor: number
}
export interface PreparationResult {
  inspection: ProfileInspection
  metadata: ProfileMetadata
  onboarding: LinkChallenge | null
}
export interface LegacyMod { path: string; size: number; sha256: string; reason: string }
export interface LegacySelection { path: string; sha256: string }
export interface LegacyBackup { backupRoot: string; files: string[] }

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
  proof_code: string
  mc_username: string
  player_uuid: string
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
  stage: 'mods' | 'java' | 'neoforge' | 'libraries' | 'assets' | 'launch'
  currentBytes: number
  totalBytes: number
}

export interface GameExitedPayload {
  profileId: string
  exitCode: number | null
}

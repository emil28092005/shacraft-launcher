export type AccountMode = 'microsoft' | 'offline'

export interface Server {
  id: string
  kicker: string
  name: string
  subtitle: string
  version: string
  composition: string
  loader: string
  profileId?: string
  disabled?: boolean
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

export interface MinecraftProfile {
  id: string
  name: string
}

export interface DeviceCodePayload {
  verificationUri: string
  userCode: string
  expiresInSeconds: number
}

export interface LoginResultPayload {
  ok: boolean
  profile: MinecraftProfile | null
  error: string | null
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

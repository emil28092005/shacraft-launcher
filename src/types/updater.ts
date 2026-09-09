/** Native updater owns endpoints, verification keys, package choice and versions. */
export interface UpdaterStatus {
  revision: number
  installedVersion: string
  testBuild: boolean
  packageFormat: 'development' | 'AppImage' | 'deb' | 'rpm' | 'MSI' | 'NSIS' | 'app' | 'unpackaged'
  phase: 'idle' | 'checking' | 'available' | 'downloading' | 'verifying' | 'installing' |
    'ready' | 'no_update' | 'unconfigured' | 'manual' | 'error'
  availableVersion: string | null
  releaseNotes: string | null
  downloadedBytes: number
  totalBytes: number | null
  canRetry: boolean
  message: string | null
}

import type { LauncherUpdateProgress, LauncherUpdateStatus } from '../types/launcher'

export interface UpdaterState {
  phase: 'loading' | 'idle' | 'checking' | 'downloading' | 'installing' | 'ready' | 'restarting'
  status: LauncherUpdateStatus | null
  progress: LauncherUpdateProgress | null
  error: string | null
  checked: boolean
}

export const initialUpdaterState: UpdaterState = {
  phase: 'loading', status: null, progress: null, error: null, checked: false,
}

type UpdaterAction =
  | { type: 'loaded'; status: LauncherUpdateStatus; checked: boolean; error?: string | null }
  | { type: 'check' }
  | { type: 'install' }
  | { type: 'progress'; progress: LauncherUpdateProgress }
  | { type: 'ready' }
  | { type: 'restart' }
  | { type: 'failed'; error: string }

export function updateBlocksOperations(state: UpdaterState): boolean {
  return ['downloading', 'installing', 'ready', 'restarting'].includes(state.phase)
}

export function updaterReducer(state: UpdaterState, action: UpdaterAction): UpdaterState {
  switch (action.type) {
    case 'loaded':
      if (updateBlocksOperations(state)) return { ...state, status: action.status }
      return { ...state, phase: action.status.stage === 'ready' || action.status.stage === 'downloading' || action.status.stage === 'installing'
        ? action.status.stage : 'idle', status: action.status,
        checked: action.checked, error: action.error ?? null }
    case 'check':
      return updateBlocksOperations(state) ? state : { ...state, phase: 'checking', error: null }
    case 'install':
      return state.phase !== 'idle' || !state.status?.supported || !state.status.version ? state
        : { ...state, phase: 'downloading', progress: null, error: null }
    case 'progress':
      if (state.phase !== 'loading' && state.phase !== 'downloading' && state.phase !== 'installing') return state
      // A delayed download event cannot revert an installation to downloading.
      if (state.phase === 'installing' && action.progress.stage === 'downloading') return state
      return { ...state, phase: action.progress.stage, progress: action.progress }
    case 'ready':
      return state.phase === 'downloading' || state.phase === 'installing'
        ? { ...state, phase: 'ready', error: null } : state
    case 'restart':
      return state.phase === 'ready' ? { ...state, phase: 'restarting', error: null } : state
    case 'failed':
      return { ...state, phase: state.phase === 'ready' || state.phase === 'restarting' ? 'ready' : 'idle', error: action.error }
  }
}

export function updatePercent(progress: LauncherUpdateProgress | null): number | null {
  if (!progress || !progress.totalBytes || progress.totalBytes <= 0 ||
      !Number.isFinite(progress.totalBytes) || !Number.isFinite(progress.downloadedBytes)) return null
  return Math.max(0, Math.min(100, Math.round(progress.downloadedBytes / progress.totalBytes * 100)))
}

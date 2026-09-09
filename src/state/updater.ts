import type { UpdaterStatus } from '../types/updater'

export type UpdaterCommand = 'status' | 'check' | 'install' | 'restart' | 'open'
export interface UpdaterState {
  status: UpdaterStatus | null
  pending: { command: UpdaterCommand; request: number } | null
  error: string | null
  blockedReason: string | null
}
export const initialUpdaterState: UpdaterState = { status: null, pending: null, error: null, blockedReason: null }

export type UpdaterAction =
  | { type: 'status'; status: UpdaterStatus }
  | { type: 'begin'; command: UpdaterCommand; request: number }
  | { type: 'settled'; request: number }
  | { type: 'failed'; request: number; error: string }
  | { type: 'blocked'; reason: string | null }

export function updaterReducer(state: UpdaterState, action: UpdaterAction): UpdaterState {
  switch (action.type) {
    case 'status':
      // Event delivery and invoke completion may arrive in either order.
      if (state.status && action.status.revision <= state.status.revision) return state
      return { ...state, status: action.status, error: null }
    case 'begin':
      return state.pending ? state : { ...state, pending: { command: action.command, request: action.request }, error: null }
    case 'settled':
      return state.pending?.request === action.request ? { ...state, pending: null } : state
    case 'failed':
      return state.pending?.request === action.request ? { ...state, pending: null, error: action.error } : state
    case 'blocked':
      return state.blockedReason === action.reason ? state : { ...state, blockedReason: action.reason }
  }
}

export function updaterMutating(state: UpdaterState): boolean {
  return state.pending?.command === 'install' || state.pending?.command === 'restart' ||
    state.status?.phase === 'downloading' || state.status?.phase === 'verifying' || state.status?.phase === 'installing'
}

export function canRunUpdater(state: UpdaterState, command: Exclude<UpdaterCommand, 'status'>): boolean {
  if (state.pending) return false
  const phase = state.status?.phase
  if (command === 'open') return phase === 'manual' || (phase === 'error' && state.status?.canRetry === false)
  if (state.blockedReason || updaterMutating(state)) return false
  if (command === 'restart') return phase === 'ready'
  if (command === 'install') return phase === 'available'
  if (phase === 'error' && state.status?.canRetry === false) return false
  return !phase || ['idle', 'available', 'no_update', 'manual', 'unconfigured', 'error'].includes(phase)
}

export function updaterPercent(status: UpdaterStatus | null): number | null {
  if (!status || status.totalBytes === null || !Number.isFinite(status.totalBytes) || status.totalBytes <= 0 ||
      !Number.isFinite(status.downloadedBytes) || status.downloadedBytes < 0) return null
  return Math.min(100, Math.floor(100 * status.downloadedBytes / status.totalBytes))
}

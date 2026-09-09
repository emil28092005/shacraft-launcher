import type { GameExitedPayload, InstallProgressPayload } from '../types/launcher'

export type GameOperation =
  | { phase: 'idle' }
  | { phase: 'syncing' | 'launching' | 'running'; profileId: string }
  | { phase: 'installing'; profileId: string; progress: InstallProgressPayload | null }

export interface GameState {
  operation: GameOperation
  error: string | null
}

export type GameAction =
  | { type: 'sync'; profileId: string }
  | { type: 'synced'; profileId: string }
  | { type: 'repaired'; profileId: string }
  | { type: 'install'; profileId: string }
  | { type: 'progress'; progress: InstallProgressPayload }
  | { type: 'launch'; profileId: string }
  | { type: 'started'; profileId: string }
  | { type: 'exited'; result: GameExitedPayload }
  | { type: 'failed'; error: string }

export const initialGameState: GameState = { operation: { phase: 'idle' }, error: null }

export function gameReducer(state: GameState, action: GameAction): GameState {
  const operation = state.operation
  switch (action.type) {
    case 'sync':
    case 'install':
      if (operation.phase !== 'idle') return state
      return {
        operation: action.type === 'sync'
          ? { phase: 'syncing', profileId: action.profileId }
          : { phase: 'installing', profileId: action.profileId, progress: null },
        error: null,
      }
    case 'synced':
      return operation.phase === 'syncing' && operation.profileId === action.profileId
        ? initialGameState : state
    case 'repaired':
      return operation.phase === 'installing' && operation.profileId === action.profileId ? initialGameState : state
    case 'progress':
      if (operation.phase === 'installing' && action.progress.stage === 'launch') return { ...state, operation: { phase: 'launching', profileId: operation.profileId } }
      return operation.phase === 'installing'
        ? { ...state, operation: { ...operation, progress: action.progress } } : state
    case 'launch':
      return operation.phase === 'installing' && operation.profileId === action.profileId
        ? { ...state, operation: { phase: 'launching', profileId: action.profileId } } : state
    case 'started':
      // A fast-exiting child can emit game-exited before invoke resolves.
      return (operation.phase === 'launching' || operation.phase === 'installing') && operation.profileId === action.profileId
        ? { ...state, operation: { phase: 'running', profileId: action.profileId } } : state
    case 'exited':
      if ((operation.phase !== 'launching' && operation.phase !== 'running' && operation.phase !== 'installing') ||
          operation.profileId !== action.result.profileId) return state
      return {
        operation: { phase: 'idle' },
        error: action.result.exitCode === 0 ? null
          : action.result.exitCode === null ? 'Игра завершилась без кода выхода. Проверьте журнал игры.'
            : `Игра завершилась с кодом ${action.result.exitCode}. Проверьте журнал игры.`,
      }
    case 'failed':
      return { operation: { phase: 'idle' }, error: action.error }
  }
}

export const installStageLabels: Record<InstallProgressPayload['stage'], string> = {
  mods: 'Обновляем сборку',
  launch: 'Запускаем игру',
  java: 'Готовим Java',
  neoforge: 'Устанавливаем NeoForge',
  libraries: 'Скачиваем библиотеки',
  assets: 'Скачиваем ресурсы игры',
}

export function installPercent(progress: InstallProgressPayload | null): number | null {
  if (!progress || progress.totalBytes <= 0 || !Number.isFinite(progress.totalBytes) ||
      !Number.isFinite(progress.currentBytes)) return null
  return Math.max(0, Math.min(100, Math.round(progress.currentBytes / progress.totalBytes * 100)))
}

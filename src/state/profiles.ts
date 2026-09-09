import type { ProfileInspection } from '../types/launcher'

export type ProfileState =
  | { status: 'checking'; inspection: null; error: null }
  | { status: 'checked'; inspection: ProfileInspection; error: null }
  | { status: 'error'; inspection: null; error: string }

type ProfileAction =
  | { type: 'check'; profileId: string }
  | { type: 'checked'; profileId: string; inspection: ProfileInspection }
  | { type: 'failed'; profileId: string; error: string }

export function profilesReducer(
  profiles: Record<string, ProfileState>, action: ProfileAction,
): Record<string, ProfileState> {
  const next: ProfileState = action.type === 'check'
    ? { status: 'checking', inspection: null, error: null }
    : action.type === 'checked'
      ? { status: 'checked', inspection: action.inspection, error: null }
      : { status: 'error', inspection: null, error: action.error }
  return { ...profiles, [action.profileId]: next }
}

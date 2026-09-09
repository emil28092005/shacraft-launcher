import { useEffect, useReducer, useRef, useState } from 'react'
import { servers } from '../data/servers'
import { errorMessage } from '../services/async'
import { isNative, native, watchGame } from '../services/native'
import { gameReducer, initialGameState } from '../state/game'
import { profilesReducer } from '../state/profiles'
import type { JavaInstallation, NativeHost, ProfileMetadata, PreparationResult } from '../types/launcher'

export function useLauncher() {
  const [host, setHost] = useState<NativeHost | null>(null)
  const [java, setJava] = useState<JavaInstallation | null | undefined>(undefined)
  const [environmentError, setEnvironmentError] = useState<string | null>(null)
  const [profiles, updateProfile] = useReducer(profilesReducer, {})
  const [game, dispatch] = useReducer(gameReducer, initialGameState)
  const [eventsReady, setEventsReady] = useState(false)
  const busy = useRef(false)
  const [metadata, setMetadata] = useState<Record<string, ProfileMetadata>>({})

  useEffect(() => {
    if (!isNative()) return
    let active = true
    const subscription = watchGame({
      progress: (progress) => { if (active) dispatch({ type: 'progress', progress }) },
      exited: (result) => { if (active) dispatch({ type: 'exited', result }) },
    })
    void subscription.ready.then(() => {
      if (active) setEventsReady(true)
    }).catch((reason: unknown) => {
      if (active) setEnvironmentError(errorMessage(reason, 'Не удалось подключить события игры'))
    })
    void native.host().then((value) => {
      if (active) setHost(value)
    }).catch((reason: unknown) => {
      if (active) setEnvironmentError(errorMessage(reason, 'Не удалось определить каталог лаунчера'))
    })
    void native.detectJava().then((value) => {
      if (active) setJava(value)
    }).catch((reason: unknown) => {
      if (!active) return
      setJava(null)
      setEnvironmentError(errorMessage(reason, 'Не удалось проверить Java'))
    })
    for (const server of servers) {
      const profileId = server.profileId
      void native.metadata(profileId).then((value) => {
        if (active) setMetadata((old) => ({ ...old, [profileId]: value }))
      }).catch(() => { /* Missing verified metadata is shown as unknown. */ })
      updateProfile({ type: 'check', profileId })
      void native.inspectProfile(profileId).then((inspection) => {
        if (active) updateProfile({ type: 'checked', profileId, inspection })
      }).catch((reason: unknown) => {
        if (active) updateProfile({ type: 'failed', profileId, error: errorMessage(reason, 'Не удалось проверить сборку') })
      })
    }
    return () => { active = false; subscription.dispose() }
  }, [])

  const prepare = async (profileId: string, mode: 'repair' | 'play' | 'onboarding', nickname?: string): Promise<PreparationResult | null> => {
    if (!isNative() || !eventsReady || busy.current || game.operation.phase !== 'idle') return null
    busy.current = true
    dispatch({ type: 'install', profileId })
    updateProfile({ type: 'check', profileId })
    try {
      // The native command owns one snapshot and one lock across every stage.
      const result = mode === 'repair' ? await native.installGame(profileId)
        : mode === 'onboarding' ? await native.launchOnboarding(profileId, nickname ?? '')
          : await native.launchGame(profileId)
      updateProfile({ type: 'checked', profileId, inspection: result.inspection })
      setMetadata((old) => ({ ...old, [profileId]: result.metadata }))
      if (mode === 'repair') dispatch({ type: 'repaired', profileId })
      else dispatch({ type: 'started', profileId })
      return result
    } catch (reason) {
      const error = errorMessage(reason, mode === 'repair' ? 'Не удалось восстановить игру' : 'Не удалось запустить игру')
      updateProfile({ type: 'failed', profileId, error })
      dispatch({ type: 'failed', error })
      return null
    } finally { busy.current = false }
  }

  const refreshProfile = async (profileId: string) => {
    updateProfile({ type: 'check', profileId })
    try { updateProfile({ type: 'checked', profileId, inspection: await native.inspectProfile(profileId) }) }
    catch (reason) { updateProfile({ type: 'failed', profileId, error: errorMessage(reason, 'Не удалось проверить сборку') }) }
  }

  return { host, java, environmentError, profiles, metadata, game, eventsReady, refreshProfile,
    repair: (profileId: string) => prepare(profileId, 'repair'),
    launch: (profileId: string) => prepare(profileId, 'play'),
    onboard: (profileId: string, nickname: string) => prepare(profileId, 'onboarding', nickname),
  }
}

import { useEffect, useReducer, useRef, useState } from 'react'
import { servers } from '../data/servers'
import { errorMessage } from '../services/async'
import { isNative, native, watchGame } from '../services/native'
import { gameReducer, initialGameState } from '../state/game'
import { profilesReducer } from '../state/profiles'
import type { JavaInstallation, NativeHost } from '../types/launcher'

export function useLauncher() {
  const [host, setHost] = useState<NativeHost | null>(null)
  const [java, setJava] = useState<JavaInstallation | null | undefined>(undefined)
  const [environmentError, setEnvironmentError] = useState<string | null>(null)
  const [profiles, updateProfile] = useReducer(profilesReducer, {})
  const [game, dispatch] = useReducer(gameReducer, initialGameState)
  const [eventsReady, setEventsReady] = useState(false)
  const busy = useRef(false)

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
      updateProfile({ type: 'check', profileId })
      void native.inspectProfile(profileId).then((inspection) => {
        if (active) updateProfile({ type: 'checked', profileId, inspection })
      }).catch((reason: unknown) => {
        if (active) updateProfile({ type: 'failed', profileId, error: errorMessage(reason, 'Не удалось проверить сборку') })
      })
    }
    return () => { active = false; subscription.dispose() }
  }, [])

  const repair = async (profileId: string) => {
    if (!isNative() || busy.current || game.operation.phase !== 'idle' || profiles[profileId]?.status === 'checking') return
    busy.current = true
    dispatch({ type: 'sync', profileId })
    // A repair may replace only some files before failing. Never keep an older
    // up-to-date inspection as permission to launch that partial installation.
    updateProfile({ type: 'check', profileId })
    try {
      const result = await native.syncProfile(profileId)
      updateProfile({ type: 'checked', profileId,
        inspection: { root: result.root, managedFiles: result.downloadedFiles + result.reusedFiles,
          missingFiles: 0, mismatchedFiles: 0, upToDate: true },
      })
      dispatch({ type: 'synced', profileId })
    } catch (reason) {
      const error = errorMessage(reason, 'Не удалось синхронизировать сборку')
      updateProfile({ type: 'failed', profileId, error })
      dispatch({ type: 'failed', error })
    } finally {
      busy.current = false
    }
  }

  const launch = async (profileId: string) => {
    if (!isNative() || !eventsReady || busy.current || game.operation.phase !== 'idle') return
    busy.current = true
    dispatch({ type: 'sync', profileId })
    updateProfile({ type: 'check', profileId })
    try {
      // Reconcile the current signed modpack before every Play, even when a
      // previous inspection succeeded. Game installation alone omits mods.
      const synced = await native.syncProfile(profileId)
      updateProfile({ type: 'checked', profileId, inspection: {
        root: synced.root, managedFiles: synced.downloadedFiles + synced.reusedFiles,
        missingFiles: 0, mismatchedFiles: 0, upToDate: true,
      } })
      dispatch({ type: 'synced', profileId })
      dispatch({ type: 'install', profileId })
      await native.installGame(profileId)
      dispatch({ type: 'launch', profileId })
      await native.launchGame(profileId)
      dispatch({ type: 'started', profileId })
    } catch (reason) {
      const error = errorMessage(reason, 'Не удалось запустить игру')
      updateProfile({ type: 'failed', profileId, error })
      dispatch({ type: 'failed', error })
    } finally {
      busy.current = false
    }
  }

  return { host, java, environmentError, profiles, game, eventsReady, repair, launch }
}

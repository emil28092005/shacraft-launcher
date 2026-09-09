import { useEffect, useReducer, useRef, useState } from 'react'
import { errorMessage, singleFlight } from '../services/async'
import { isNative, native, watchLauncherUpdate } from '../services/native'
import { initialUpdaterState, updateBlocksOperations, updaterReducer } from '../state/updater'

// StrictMode re-runs effects; share the pending native check without installing
// anything. Manual checks always make a fresh request.
const startupCheck = singleFlight(async () => {
  const status = await native.updateStatus()
  if (!status.supported || (status.stage && ['downloading', 'installing', 'ready'].includes(status.stage))) return { status, checked: false, error: null }
  try {
    return { status: await native.checkUpdate(), checked: true, error: null }
  } catch (error) {
    return { status, checked: false, error: errorMessage(error, 'Не удалось проверить обновления. Повторите попытку.') }
  }
})

export function useLauncherUpdate(blocked: boolean) {
  const [state, dispatch] = useReducer(updaterReducer, initialUpdaterState)
  const [eventsReady, setEventsReady] = useState(false)
  const [eventError, setEventError] = useState<string | null>(null)
  const pending = useRef(false)
  const mounted = useRef(false)

  useEffect(() => {
    if (!isNative()) return
    let active = true
    mounted.current = true
    const subscription = watchLauncherUpdate((progress) => {
      if (active) dispatch({ type: 'progress', progress })
    })
    void subscription.ready.then(() => {
      if (active) { setEventsReady(true); setEventError(null) }
    }).catch((reason: unknown) => {
      if (active) setEventError(errorMessage(reason, 'Не удалось подключить события обновления. Перезапустите лаунчер.'))
    })
    pending.current = true
    void startupCheck().then((result) => {
      if (active) dispatch({ type: 'loaded', ...result })
    }).catch((reason: unknown) => {
      if (active) dispatch({ type: 'failed', error: errorMessage(reason, 'Не удалось проверить обновления. Повторите попытку.') })
    }).finally(() => { if (active) pending.current = false })
    return () => { active = false; mounted.current = false; subscription.dispose() }
  }, [])

  const check = async () => {
    if (!isNative() || pending.current || updateBlocksOperations(state)) return
    pending.current = true
    dispatch({ type: 'check' })
    try {
      const status = await native.checkUpdate()
      if (mounted.current) dispatch({ type: 'loaded', status, checked: true })
    } catch (reason) {
      if (mounted.current) dispatch({ type: 'failed', error: errorMessage(reason, 'Не удалось проверить обновления. Повторите попытку.') })
    } finally { pending.current = false }
  }

  const install = async () => {
    if (!isNative() || pending.current || blocked || !eventsReady || state.phase !== 'idle' ||
        !state.status?.supported || !state.status.version) return
    pending.current = true
    dispatch({ type: 'install' })
    try {
      await native.installUpdate()
      if (mounted.current) dispatch({ type: 'ready' })
    } catch (reason) {
      if (mounted.current) dispatch({ type: 'failed', error: errorMessage(reason, 'Не удалось установить обновление. Повторите попытку.') })
    } finally { pending.current = false }
  }

  const restart = async () => {
    if (!isNative() || pending.current || blocked || state.phase !== 'ready') return
    pending.current = true
    dispatch({ type: 'restart' })
    try {
      await native.restartAfterUpdate()
    } catch (reason) {
      if (mounted.current) dispatch({ type: 'failed', error: errorMessage(reason, 'Не удалось перезапустить лаунчер. Закройте его и откройте снова.') })
    } finally { pending.current = false }
  }

  return { state, eventsReady, eventError, blocked, locksOperations: updateBlocksOperations(state), check, install, restart }
}

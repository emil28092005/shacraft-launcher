import { errorMessage } from './async'
import { canRunUpdater, initialUpdaterState, updaterReducer } from '../state/updater'
import type { UpdaterAction, UpdaterCommand, UpdaterState } from '../state/updater'
import type { UpdaterStatus } from '../types/updater'

export interface UpdaterApi {
  status: () => Promise<UpdaterStatus>
  check: () => Promise<UpdaterStatus>
  install: () => Promise<UpdaterStatus>
  restart: () => Promise<void>
  open: () => Promise<void>
  watch: (receive: (status: UpdaterStatus) => void) => { ready: Promise<void>; dispose: () => void }
}

/** One window lifecycle, including StrictMode reconnects; never installs on its own. */
export function createUpdaterController(api: UpdaterApi) {
  let state = initialUpdaterState
  const subscribers = new Set<() => void>()
  let request = 0
  let generation = 0
  let connected = false
  let initialized = false
  let automaticCheckStarted = false
  let subscription: ReturnType<UpdaterApi['watch']> | null = null

  const dispatch = (action: UpdaterAction) => {
    const next = updaterReducer(state, action)
    if (next === state) return
    state = next
    subscribers.forEach((notify) => notify())
  }

  const ensureEvents = () => {
    if (!subscription) {
      const current = generation
      const created = api.watch((status) => {
        if (connected && generation === current) dispatch({ type: 'status', status })
      })
      subscription = created
      void created.ready.catch(() => {
        if (subscription === created) { created.dispose(); subscription = null }
      })
    }
    return subscription.ready
  }

  const run = async (command: Exclude<UpdaterCommand, 'status'>): Promise<boolean> => {
    if (!connected || !canRunUpdater(state, command)) return false
    const current = ++request
    const currentGeneration = generation
    dispatch({ type: 'begin', command, request: current })
    try {
      if (command !== 'open') await ensureEvents()
      // Settings/account work may have started while the listener registered.
      if (!connected || generation !== currentGeneration || (command !== 'open' && state.blockedReason)) return false
      const status = await api[command]()
      if (status) dispatch({ type: 'status', status })
      return true
    } catch (reason) {
      dispatch({ type: 'failed', request: current, error: errorMessage(reason, 'Не удалось выполнить обновление лаунчера. Повторите попытку.') })
      return false
    } finally {
      dispatch({ type: 'settled', request: current })
    }
  }

  const automaticCheck = () => {
    if (!connected || !initialized || automaticCheckStarted || !canRunUpdater(state, 'check')) return
    automaticCheckStarted = true
    void run('check')
  }

  const connect = () => {
    connected = true
    const current = ++generation
    const read = ++request
    // A user-started native operation can outlive a UI reconnect.
    const ownsPending = state.pending === null
    if (ownsPending) dispatch({ type: 'begin', command: 'status', request: read })
    void ensureEvents().then(() => connected && generation === current ? api.status() : null).then((status) => {
      if (!connected || generation !== current) return
      if (status) dispatch({ type: 'status', status })
      initialized = true
    }).catch((reason) => {
      if (connected && generation === current && ownsPending) dispatch({ type: 'failed', request: read,
        error: errorMessage(reason, 'Не удалось прочитать состояние обновления лаунчера.') })
    }).finally(() => {
      if (ownsPending) dispatch({ type: 'settled', request: read })
      if (connected && generation === current) automaticCheck()
    })
    return () => {
      if (generation !== current) return
      connected = false
      initialized = false
      generation++
      subscription?.dispose()
      subscription = null
      if (ownsPending) dispatch({ type: 'settled', request: read })
    }
  }

  return {
    snapshot: (): UpdaterState => state,
    subscribe: (notify: () => void) => { subscribers.add(notify); return () => { subscribers.delete(notify) } },
    connect,
    setBlockedReason: (reason: string | null) => { dispatch({ type: 'blocked', reason }); automaticCheck() },
    check: () => { automaticCheckStarted = true; return run('check') },
    install: () => run('install'),
    restart: () => run('restart'),
    open: () => run('open'),
  }
}

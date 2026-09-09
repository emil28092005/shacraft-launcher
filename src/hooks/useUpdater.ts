import { useEffect, useRef, useSyncExternalStore } from 'react'
import { isNative, native, watchUpdater } from '../services/native'
import { createUpdaterController } from '../services/updater'
import { initialUpdaterState, updaterMutating } from '../state/updater'

export function useUpdater(blockedReason: string | null) {
  const controller = useRef(createUpdaterController({
    status: native.updaterStatus,
    check: native.checkUpdater,
    install: native.installUpdater,
    restart: native.restartUpdater,
    open: native.openUpdaterRelease,
    watch: watchUpdater,
  })).current
  const state = useSyncExternalStore(controller.subscribe, controller.snapshot, () => initialUpdaterState)
  useEffect(() => { controller.setBlockedReason(blockedReason) }, [controller, blockedReason])
  useEffect(() => {
    if (isNative()) return controller.connect()
  }, [controller])
  return { state, mutating: updaterMutating(state),
    check: () => { void controller.check() },
    install: () => { void controller.install() },
    restart: () => { void controller.restart() },
    openRelease: () => { void controller.open() },
  }
}

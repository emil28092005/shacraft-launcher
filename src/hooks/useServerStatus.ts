import { useEffect, useState } from 'react'
import { isNative, native } from '../services/native'
import type { ServerStatus } from '../types/launcher'

export function useServerStatus(profileId: string) {
  const [status, setStatus] = useState<ServerStatus | null>(null)
  useEffect(() => {
    setStatus(null)
    if (!isNative()) return
    let active = true
    let timer: ReturnType<typeof setTimeout> | undefined
    const refresh = async () => {
      try {
        const next = await native.serverStatus(profileId)
        if (active) setStatus(next)
      } catch {
        if (active) setStatus({ online: null, max: null, reachable: false })
      } finally {
        // Schedule after completion; slow network calls never overlap.
        if (active) timer = setTimeout(() => { void refresh() }, 30_000)
      }
    }
    void refresh()
    return () => { active = false; clearTimeout(timer) }
  }, [profileId])
  return status
}

import { useEffect, useRef, useState } from 'react'
import { errorMessage } from '../services/async'
import { isNative, native, watchAccount } from '../services/native'
import type { DeviceCodePayload, MinecraftProfile } from '../types/launcher'

export function useAccount() {
  // undefined = restoring saved account; null = signed out.
  const [account, setAccount] = useState<MinecraftProfile | null | undefined>(isNative() ? undefined : null)
  const [code, setCode] = useState<DeviceCodePayload | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [eventsReady, setEventsReady] = useState(false)
  const pending = useRef(false)

  useEffect(() => {
    if (!isNative()) return
    let active = true
    let changed = false
    const subscription = watchAccount({
      code: (payload) => { if (active) setCode(payload) },
      result: (payload) => {
        if (!active) return
        changed = true
        pending.current = false
        setBusy(false)
        setCode(null)
        if (payload.ok && payload.profile) {
          setAccount(payload.profile)
          setError(null)
        } else setError(payload.error || 'Не удалось войти через Microsoft')
      },
    })
    void subscription.ready.then(() => {
      if (active) setEventsReady(true)
    }).catch((reason: unknown) => {
      if (active) setError(errorMessage(reason, 'Не удалось подготовить вход через Microsoft'))
    })
    void native.getAccount().then((profile) => {
      if (active && !changed) setAccount(profile)
    }).catch((reason: unknown) => {
      if (!active || changed) return
      setAccount(null)
      setError(errorMessage(reason, 'Не удалось восстановить аккаунт'))
    })
    return () => { active = false; subscription.dispose() }
  }, [])

  const login = async () => {
    if (!isNative() || !eventsReady || pending.current || account === undefined) return
    pending.current = true
    setBusy(true)
    setCode(null)
    setError(null)
    try {
      await native.startLogin()
    } catch (reason) {
      pending.current = false
      setBusy(false)
      setError(errorMessage(reason, 'Не удалось начать вход через Microsoft'))
    }
  }

  const logout = async () => {
    if (!isNative() || pending.current) return
    pending.current = true
    setBusy(true)
    setError(null)
    try {
      await native.logout()
      setAccount(null)
    } catch (reason) {
      setError(errorMessage(reason, 'Не удалось выйти из аккаунта'))
    } finally {
      pending.current = false
      setBusy(false)
    }
  }

  return { account, code, error, busy, eventsReady, login, logout }
}

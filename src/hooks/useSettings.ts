import { useEffect, useRef, useState } from 'react'
import { createSaveIntent, createSerialQueue, errorMessage } from '../services/async'
import { isNative, native } from '../services/native'
import { defaultSettings } from '../state/settings'
import type { LauncherSettings } from '../types/launcher'

export function useSettings() {
  const [settings, setSettings] = useState(defaultSettings)
  const [loaded, setLoaded] = useState(!isNative())
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [loadAttempt, setLoadAttempt] = useState(0)
  const current = useRef(settings)
  const durable = useRef(settings)
  const intent = useRef(createSaveIntent<LauncherSettings>())
  const queue = useRef(createSerialQueue())

  useEffect(() => {
    if (!isNative()) return
    let active = true
    setError(null)
    native.loadSettings().then((value) => {
      if (!active) return
      current.current = value
      durable.current = value
      setSettings(value)
      setLoaded(true)
    }).catch((reason: unknown) => {
      if (active) setError(errorMessage(reason, 'Не удалось прочитать настройки'))
    })
    return () => { active = false }
  }, [loadAttempt])

  const save = (patch: Partial<LauncherSettings>) => {
    if (!loaded) return
    const next = { ...current.current, ...patch }
    current.current = next
    setSettings(next)
    setError(null)
    if (!isNative()) return
    const request = intent.current.begin(next)
    setSaving(true)
    void queue.current.enqueue(() => native.saveSettings(next)).then((value) => {
      durable.current = value
      intent.current.succeeded(request)
      if (intent.current.isLatest(request)) {
        current.current = value
        setSettings(value)
      }
    }).catch((reason: unknown) => {
      if (!intent.current.isLatest(request)) return
      intent.current.failed(request)
      current.current = durable.current
      setSettings(durable.current)
      setError(errorMessage(reason, 'Не удалось сохранить настройки'))
    }).finally(() => {
      if (intent.current.isLatest(request)) setSaving(false)
    })
  }

  return {
    settings, loaded, saving, error,
    updateRam: (memoryGb: number) => save({ memoryMb: memoryGb * 1024 }),
    retry: () => {
      if (!loaded) setLoadAttempt((attempt) => attempt + 1)
      else {
        const failed = intent.current.retryValue()
        if (failed) save(failed)
      }
    },
  }
}

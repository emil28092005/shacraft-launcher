import { useEffect, useRef, useState } from 'react'
import { createSerialQueue, errorMessage } from '../services/async'
import { isNative, native } from '../services/native'
import { defaultSettings, isValidNickname } from '../state/settings'
import type { AccountMode, LauncherSettings } from '../types/launcher'

export function useSettings() {
  const [settings, setSettings] = useState(defaultSettings)
  const [nickname, setNickname] = useState(defaultSettings.nickname)
  const [loaded, setLoaded] = useState(!isNative())
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [nicknameError, setNicknameError] = useState<string | null>(null)
  const [loadAttempt, setLoadAttempt] = useState(0)
  const current = useRef(settings)
  const durable = useRef(settings)
  const revision = useRef(0)
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
      setNickname(value.nickname)
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
    const requestRevision = ++revision.current
    setSaving(true)
    void queue.current.enqueue(() => native.saveSettings(next)).then((value) => {
      durable.current = value
      if (revision.current === requestRevision) {
        current.current = value
        setSettings(value)
      }
    }).catch((reason: unknown) => {
      if (revision.current !== requestRevision) return
      current.current = durable.current
      setSettings(durable.current)
      setError(errorMessage(reason, 'Не удалось сохранить настройки'))
    }).finally(() => {
      if (revision.current === requestRevision) setSaving(false)
    })
  }

  const saveNickname = () => {
    if (!isValidNickname(nickname)) {
      setNicknameError('Ник: от 3 до 16 латинских букв, цифр или символов _')
      return
    }
    setNicknameError(null)
    save({ nickname })
  }

  return {
    settings, nickname, loaded, saving, error, nicknameError,
    updateRam: (memoryGb: number) => save({ memoryMb: memoryGb * 1024 }),
    updateMode: (accountMode: AccountMode) => save({ accountMode }),
    updateNickname: (value: string) => { setNickname(value); setNicknameError(null) },
    saveNickname,
    retry: () => {
      if (!loaded) setLoadAttempt((attempt) => attempt + 1)
      else if (isValidNickname(nickname)) save({ nickname })
      else save({})
    },
  }
}

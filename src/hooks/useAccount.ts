import { useEffect, useRef, useState } from 'react'
import { createRequestScope, errorMessage } from '../services/async'
import { isNative, native } from '../services/native'
import { linkedNickname, validCredentials } from '../state/account'
import { isValidNickname } from '../state/settings'
import type { ShaCraftAccount } from '../types/launcher'

interface PendingLink {
  challengeId: number
  expiresAt: number
  isCurrent: () => boolean
}

export function useAccount() {
  // undefined = restoring saved account; null = signed out.
  const [account, setAccount] = useState<ShaCraftAccount | null | undefined>(isNative() ? undefined : null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [recoveryCodes, setRecoveryCodes] = useState<string[]>([])
  const [challenge, setChallenge] = useState<PendingLink | null>(null)
  const [linkMessage, setLinkMessage] = useState<string | null>(null)
  const pending = useRef(false)
  const requests = useRef(createRequestScope())

  useEffect(() => {
    if (!isNative()) return
    let active = true
    void native.getAccount().then((value) => {
      if (active) setAccount(value)
    }).catch((reason: unknown) => {
      if (!active) return
      setAccount(null)
      setError(errorMessage(reason, 'Не удалось восстановить аккаунт ShaCraft'))
    })
    return () => { active = false; requests.current.invalidate() }
  }, [])

  useEffect(() => {
    if (!challenge) return
    let active = true
    let timer: ReturnType<typeof setTimeout> | undefined
    const current = () => active && challenge.isCurrent()
    const poll = async () => {
      if (!current()) return
      if (Date.now() >= challenge.expiresAt) {
        setChallenge(null)
        setLinkMessage('Срок проверки истёк. Начните привязку ещё раз.')
        return
      }
      try {
        const result = await native.linkStatus(challenge.challengeId)
        if (!current()) return
        if (result.status === 'verified') {
          const refreshed = await native.getAccount()
          if (!current()) return
          setAccount(refreshed)
          setChallenge(null)
          setLinkMessage(linkedNickname(refreshed) ? 'Ник подтверждён.' : 'Не удалось подтвердить привязку. Войдите снова.')
        } else if (result.status === 'expired' || result.status === 'conflict') {
          setChallenge(null)
          setLinkMessage(result.detail || 'Проверка завершилась. Попробуйте ещё раз.')
        } else {
          // One request at a time; dispose and logout cancel future polling.
          timer = setTimeout(() => { void poll() }, 3_000)
        }
      } catch (reason) {
        if (!current()) return
        setChallenge(null)
        setLinkMessage(errorMessage(reason, 'Не удалось проверить ник'))
      }
    }
    timer = setTimeout(() => { void poll() }, 3_000)
    return () => { active = false; clearTimeout(timer) }
  }, [challenge])

  const authenticate = async (username: string, password: string, register: boolean) => {
    if (pending.current || account === undefined) return false
    if (!validCredentials(username, password)) {
      setError('Логин: 3–32 латинских буквы, цифры или _; пароль: 3–128 символов')
      return false
    }
    if (!isNative()) {
      setError('Вход в ShaCraft доступен в приложении лаунчера')
      return false
    }
    pending.current = true
    requests.current.invalidate()
    const currentRequest = requests.current.capture()
    setBusy(true)
    setError(null)
    try {
      const result = await native.authenticate(username, password, register)
      if (!currentRequest()) return false
      setAccount(result.account)
      setChallenge(null)
      setLinkMessage(null)
      setRecoveryCodes(result.recoveryCodes)
      return true
    } catch (reason) {
      setError(errorMessage(reason, register ? 'Не удалось зарегистрироваться' : 'Не удалось войти'))
      return false
    } finally {
      pending.current = false
      setBusy(false)
    }
  }

  const logout = async () => {
    if (!isNative() || pending.current) return
    pending.current = true
    requests.current.invalidate()
    setChallenge(null)
    setLinkMessage(null)
    setBusy(true)
    setError(null)
    try {
      await native.logout()
      setAccount(null)
      setRecoveryCodes([])
    } catch (reason) {
      setError(errorMessage(reason, 'Не удалось выйти из ShaCraft'))
    } finally {
      pending.current = false
      setBusy(false)
    }
  }

  const startLink = async (nickname: string) => {
    if (!isNative() || pending.current || challenge || !account) return
    if (!isValidNickname(nickname)) {
      setLinkMessage('Ник: 3–16 латинских букв, цифр или _')
      return
    }
    pending.current = true
    setBusy(true)
    setLinkMessage('Создаём проверку…')
    requests.current.invalidate()
    const currentRequest = requests.current.capture()
    try {
      const started = await native.startLink(nickname)
      if (!currentRequest()) return
      setLinkMessage(started.registered_on_server
        ? 'Зайдите на Aeronautics с этим ником и выполните /login.'
        : 'Зайдите на Aeronautics с этим ником и выполните /register.')
      setChallenge({ challengeId: started.challenge_id,
        expiresAt: Date.now() + started.expires_in_seconds * 1000, isCurrent: currentRequest })
    } catch (reason) {
      setLinkMessage(errorMessage(reason, 'Не удалось начать привязку'))
    } finally {
      pending.current = false
      setBusy(false)
    }
  }

  return {
    account, error, busy, recoveryCodes, linkMessage, linking: challenge !== null,
    linkedNickname: linkedNickname(account), authenticate, logout, startLink,
    clearError: () => setError(null),
    acknowledgeRecoveryCodes: () => setRecoveryCodes([]),
  }
}

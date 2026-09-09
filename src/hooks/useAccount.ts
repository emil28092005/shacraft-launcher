import { useCallback, useEffect, useRef, useState } from 'react'
import type { Feedback } from '../components/FeedbackDialog'
import { createRequestScope, errorMessage } from '../services/async'
import { isNative, native } from '../services/native'
import { linkedNickname, validCredentials } from '../state/account'
import { isValidNickname } from '../state/settings'
import type { ShaCraftAccount } from '../types/launcher'

export function useAccount() {
  // undefined = restoring saved account; null = signed out.
  const [account, setAccount] = useState<ShaCraftAccount | null | undefined>(isNative() ? undefined : null)
  const [error, setError] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [recoveryCodes, setRecoveryCodes] = useState<string[]>([])
  const [linkMessage, setLinkMessage] = useState<string | null>(null)
  const [feedback, setFeedback] = useState<Feedback | null>(null)
  const reportLink = useCallback((message: string, kind: Feedback['kind'] = 'info') => {
    setLinkMessage(message)
    setFeedback({ kind, title: kind === 'error' ? 'Не удалось привязать ник' : 'Привязка игрового ника', message })
  }, [])
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
    if (pending.current) return
    if (!isNative()) {
      reportLink('Привязка доступна в приложении лаунчера.', 'error')
      return
    }
    if (!account) {
      reportLink('Войдите в аккаунт ShaCraft, затем повторите привязку.', 'error')
      return
    }
    nickname = nickname.trim()
    if (!isValidNickname(nickname)) {
      reportLink('Ник: 3–16 латинских букв, цифр или _', 'error')
      return
    }
    pending.current = true
    setBusy(true)
    setError(null)
    reportLink('Проверяем аккаунт и закрепляем ник…')
    requests.current.invalidate()
    const currentRequest = requests.current.capture()
    try {
      const refreshed = await native.getAccount()
      if (!currentRequest()) return
      setAccount(refreshed)
      if (!refreshed) {
        reportLink('Сессия завершена или аккаунт удалён. Войдите в ShaCraft снова; если аккаунт удалён, создайте новый.', 'error')
        return
      }
      const linkedAccount = await native.claimNickname(nickname)
      if (!currentRequest()) return
      setAccount(linkedAccount)
      const confirmed = linkedNickname(linkedAccount)
      reportLink(confirmed ? `Ник ${confirmed} закреплён за аккаунтом. Теперь можно запускать игру.`
        : 'Не удалось получить закреплённый ник. Войдите снова.', confirmed ? 'success' : 'error')
    } catch (reason) {
      if (currentRequest()) reportLink(errorMessage(reason, 'Не удалось начать привязку'), 'error')
    } finally {
      pending.current = false
      setBusy(false)
    }
  }

  return {
    account, error, busy, recoveryCodes, linkMessage, linking: false,
    feedback, dismissFeedback: () => setFeedback(null),
    linkedNickname: linkedNickname(account), authenticate, logout, startLink,
    clearError: () => setError(null),
    acknowledgeRecoveryCodes: () => setRecoveryCodes([]),
  }
}

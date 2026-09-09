import { equal, match, doesNotMatch } from 'node:assert/strict'
import { test } from 'node:test'
import { renderToStaticMarkup } from 'react-dom/server'
import { AccountSettings } from './AccountSettings'
import { RecoveryCodesModal } from './RecoveryCodesModal'
import type { useAccount } from '../hooks/useAccount'

function session(): ReturnType<typeof useAccount> {
  return {
    account: null, error: null, busy: false, recoveryCodes: [], linkMessage: null,
    feedback: null, dismissFeedback: () => {},
    linking: false, linkedNickname: null, authenticate: async () => true,
    logout: async () => {}, startLink: async () => {}, clearError: () => {},
    acknowledgeRecoveryCodes: () => {},
  }
}

test('signed-out settings expose ShaCraft login/registration and the current credential limits', () => {
  const html = renderToStaticMarkup(<AccountSettings session={session()} locked={false} />)
  match(html, /Логин ShaCraft/)
  match(html, /Нет аккаунта — регистрация/)
  match(html, /minLength="3" maxLength="32"/)
  match(html, /minLength="3" maxLength="128"/)
  doesNotMatch(html, /Microsoft|Offline|Тип аккаунта|Игровой ник/)
})

test('linked identity is displayed read-only while an unlinked account offers verification', () => {
  const account = { username: 'website_login', links: [{ server_id: 'aoc', mc_username: 'Bound_Name' }] }
  const linked = renderToStaticMarkup(<AccountSettings session={{ ...session(), account, linkedNickname: 'Bound_Name' }} locked={false} />)
  match(linked, /Bound_Name/)
  doesNotMatch(linked, /<input|Привязать ник/)
  const unlinked = renderToStaticMarkup(<AccountSettings session={{ ...session(), account: { username: 'website_login', links: [] } }} locked={false} />)
  match(unlinked, /Привязать ник/)
  match(unlinked, /Aeronautics/)
})

test('recovery codes render only until explicitly acknowledged', () => {
  const codes = ['TEST-RECOVERY-ONE', 'TEST-RECOVERY-TWO']
  const html = renderToStaticMarkup(<RecoveryCodesModal codes={codes} onAcknowledge={() => {}} />)
  match(html, /Коды восстановления/)
  match(html, /TEST-RECOVERY-ONE/)
  match(html, /TEST-RECOVERY-TWO/)
  match(html, /Я сохранил коды/)
  equal(renderToStaticMarkup(<RecoveryCodesModal codes={[]} onAcknowledge={() => {}} />), '')
})

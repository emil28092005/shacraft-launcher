import { equal, match, doesNotMatch } from 'node:assert/strict'
import { test } from 'node:test'
import { renderToStaticMarkup } from 'react-dom/server'
import { AccountSettings } from './AccountSettings'
import { RecoveryCodesModal } from './RecoveryCodesModal'
import { LegacyModsDialog } from './LegacyModsDialog'
import { PlayDock } from './PlayDock'
import { launchAccess } from '../state/account'
import { servers } from '../data/servers'
import type { useAccount } from '../hooks/useAccount'

function session(): ReturnType<typeof useAccount> {
  return {
    account: null, error: null, busy: false, recoveryCodes: [], linkMessage: null,
    linking: false, linkedNickname: null, authenticate: async () => true,
    logout: async () => {}, startLink: async () => {}, clearError: () => {},
    acceptChallenge: () => {},
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

test('onboarding is visible only for a signed-in unlinked account and needs an explicit nickname', () => {
  const onOnboard = async () => {}
  const unlinked = { username: 'website_login', links: [] }
  const html = renderToStaticMarkup(<AccountSettings session={{ ...session(), account: unlinked }} locked={false} onOnboard={onOnboard} />)
  match(html, /Установить и войти для подтверждения/)
  match(html, /<button[^>]*type="button"[^>]*disabled=""[^>]*><span>Установить и войти для подтверждения/)
  match(html, /minLength="3" maxLength="16" pattern="\[A-Za-z0-9_\]\{3,16\}" required=""/)
  equal(launchAccess(unlinked), 'link')

  const signedOut = renderToStaticMarkup(<AccountSettings session={session()} locked={false} onOnboard={onOnboard} />)
  doesNotMatch(signedOut, /Установить и войти для подтверждения/)
  const linked = { username: 'website_login', links: [{ server_id: 'aoc', mc_username: 'Bound_Name' }] }
  const linkedHtml = renderToStaticMarkup(<AccountSettings session={{ ...session(), account: linked, linkedNickname: 'Bound_Name' }} locked={false} onOnboard={onOnboard} />)
  doesNotMatch(linkedHtml, /Установить и войти для подтверждения/)
  equal(launchAccess(linked), 'ready')
})

test('a verified pack does not make an unlinked account ready for normal play', () => {
  const access = launchAccess({ username: 'website_login', links: [] })
  const html = renderToStaticMarkup(<PlayDock server={servers[0]} operation={{ phase: 'idle' }}
    profile={{ status: 'checked', error: null, inspection: { root: '/test/profile', managedFiles: 12, missingFiles: 0, mismatchedFiles: 0, upToDate: true, legacyFiles: 2 } }}
    memoryGb={6} native needsLogin={access === 'login'} needsLink={access === 'link'} error={null}
    label="Привязать ник" primaryDisabled={false} repairDisabled={false} onPrimary={() => {}} onRepair={() => {}} onLegacy={() => {}} />)
  match(html, /Нужно привязать ник/)
  doesNotMatch(html, /Сборка готова|файлов под контролем/)
  match(html, /Проверено файлов сборки: 12/)
})

test('incomplete inspection does not claim that all pack files were verified', () => {
  const html = renderToStaticMarkup(<PlayDock server={servers[0]} operation={{ phase: 'idle' }}
    profile={{ status: 'checked', error: null, inspection: { root: '/test/profile', managedFiles: 12, missingFiles: 1, mismatchedFiles: 2, upToDate: false } }}
    memoryGb={6} native needsLogin={false} needsLink={false} error={null}
    label="Проверить" primaryDisabled={false} repairDisabled={false} onPrimary={() => {}} onRepair={() => {}} onLegacy={() => {}} />)
  match(html, /Требуется проверка/)
  match(html, /Файлов в сборке: 12/)
  doesNotMatch(html, /Проверено файлов сборки|файлов под контролем/)
})

test('legacy review opens with no selected files and remains closable while its list is loading', () => {
  const html = renderToStaticMarkup(<LegacyModsDialog profileId="aeronautics" onClose={() => {}} onChanged={() => {}} />)
  match(html, /role="dialog" aria-modal="true" aria-labelledby="legacy-title" aria-describedby="legacy-description"/)
  match(html, /По умолчанию ничего не выбрано/)
  match(html, /<button disabled="">Перенести выбранные \(0\) в резервную копию<\/button>/)
  match(html, /<button>Закрыть<\/button>/)
  doesNotMatch(html, /checked=""/)
})

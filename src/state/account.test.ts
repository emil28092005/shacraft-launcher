import { equal } from 'node:assert/strict'
import { test } from 'node:test'
import { launchAccess, linkedNickname, validCredentials } from './account.ts'

test('launch requires a restored ShaCraft account and its verified Aeronautics link', () => {
  equal(launchAccess(undefined), 'loading')
  equal(launchAccess(null), 'login')
  equal(launchAccess({ username: 'account', links: [] }), 'link')
  equal(launchAccess({ username: 'account', links: [{ server_id: 'create', mc_username: 'Other_Name' }] }), 'link')
  equal(launchAccess({ username: 'account', links: [{ server_id: 'aoc', mc_username: 'Verified_Name' }] }), 'ready')
})

test('identity uses snake-case account link payload and never the account login', () => {
  const account = { username: 'Local_Login', links: [
    { server_id: 'create', mc_username: 'Other_Name' },
    { server_id: 'aoc', mc_username: 'Verified_Name' },
  ] }
  equal(linkedNickname(account), 'Verified_Name')
  equal(linkedNickname({ username: 'Valid_Login', links: [] }), null)
  equal(linkedNickname({ username: 'Valid_Login', links: [{ server_id: 'aoc', mc_username: '../unsafe' }] }), null)
})

test('ShaCraft credentials preserve the server 3–32 login and 3–128 password contract', () => {
  equal(validCredentials('abc', '123'), true)
  equal(validCredentials('a'.repeat(32), 'p'.repeat(128)), true)
  equal(validCredentials('ab', '123'), false)
  equal(validCredentials('a'.repeat(33), '123'), false)
  equal(validCredentials('неверно', '123'), false)
  equal(validCredentials('account', '12'), false)
  equal(validCredentials('account', 'p'.repeat(129)), false)
})

import type { ShaCraftAccount } from '../types/launcher'
import { isValidNickname } from './settings'

/** Only the authenticated account's verified Aeronautics link selects a name. */
export function linkedNickname(account: ShaCraftAccount | null | undefined): string | null {
  const nickname = account?.links.find((link) => link.server_id === 'aoc')?.mc_username
  return nickname && isValidNickname(nickname) ? nickname : null
}

export function launchAccess(account: ShaCraftAccount | null | undefined) {
  if (account === undefined) return 'loading'
  if (account === null) return 'login'
  return linkedNickname(account) ? 'ready' : 'link'
}

export function validCredentials(username: string, password: string): boolean {
  return /^[A-Za-z0-9_]{3,32}$/.test(username) && password.length >= 3 && password.length <= 128
}

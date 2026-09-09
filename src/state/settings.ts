import type { LauncherSettings } from '../types/launcher'

export const defaultSettings: LauncherSettings = {
  memoryMb: 6 * 1024,
  nickname: 'Emil',
  accountMode: 'offline',
}

export const isValidNickname = (nickname: string) => /^[A-Za-z0-9_]{3,16}$/.test(nickname)

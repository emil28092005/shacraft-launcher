import type { Server } from '../types/launcher'

// Presentation metadata only. Rust reads install versions and managed files
// from the signed manifest; this list cannot control downloads or launch args.
export const servers: readonly [Server, ...Server[]] = [
  {
    id: 'aoc',
    kicker: 'Основная сборка',
    name: 'Aeronautics',
    subtitle: 'Строй корабли. Поднимай города в небо.',
    version: 'Версия уточняется',
    loader: 'По подписанной сборке',
    profileId: 'aeronautics',
  },
]

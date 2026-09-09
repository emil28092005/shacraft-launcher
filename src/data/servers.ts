import type { Server } from '../types/launcher'

// Presentation metadata only. Rust reads install versions and managed files
// from the signed manifest; this list cannot control downloads or launch args.
export const servers: readonly [Server, ...Server[]] = [
  {
    id: 'aoc',
    kicker: 'All of Create / сборка 2.5',
    name: 'Aeronautics',
    subtitle: 'Строй корабли. Поднимай города в небо.',
    version: '1.21.1 · NeoForge',
    composition: '250 модов',
    loader: 'NeoForge 21.1.248',
    profileId: 'aeronautics',
  },
  {
    id: 'create',
    kicker: 'На техобслуживании',
    name: 'Create',
    subtitle: 'Механизмы, фабрики и большие идеи.',
    version: '1.21.1 · NeoForge',
    composition: '41 мод',
    loader: 'NeoForge 21.1.249',
    disabled: true,
  },
]

import { Minus, Square, X } from 'lucide-react'
import logo from '../assets/shacraft-logo.png'
import type { NativeHost } from '../types/launcher'

export function Titlebar({ host }: { host: NativeHost | null }) {
  return (
    <header className="titlebar">
      <div className="brand"><img src={logo} alt="" /><span>ShaCraft</span></div>
      <div className="titlebar-drag">{host ? `Лаунчер · ${host.platform}` : 'Лаунчер'}</div>
      <div className="window-actions" aria-label="Управление окном">
        <button aria-label="Свернуть" disabled title="Управление окном пока недоступно"><Minus size={15} /></button>
        <button aria-label="Развернуть" disabled title="Управление окном пока недоступно"><Square size={12} /></button>
        <button className="close" aria-label="Закрыть" disabled title="Управление окном пока недоступно"><X size={15} /></button>
      </div>
    </header>
  )
}

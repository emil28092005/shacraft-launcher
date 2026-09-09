import { Minus, Square, X } from 'lucide-react'
import logo from '../assets/shacraft-logo.png'
import { errorMessage } from '../services/async'
import { isNative, windowControls } from '../services/native'
import type { NativeHost } from '../types/launcher'

export function Titlebar({ host, onError }: { host: NativeHost | null; onError: (message: string) => void }) {
  const control = (action: () => Promise<void>) => {
    if (isNative()) void action().catch((reason: unknown) => onError(errorMessage(reason, 'Не удалось изменить окно')))
  }
  return (
    <header className="titlebar" data-tauri-drag-region>
      <div className="brand" data-tauri-drag-region><img src={logo} alt="" data-tauri-drag-region /><span data-tauri-drag-region>ShaCraft</span></div>
      <div className="titlebar-drag" data-tauri-drag-region>{host ? `Лаунчер · ${host.platform}` : 'Лаунчер'}</div>
      <div className="window-actions" aria-label="Управление окном">
        <button aria-label="Свернуть" disabled={!isNative()} onClick={() => control(windowControls.minimize)}><Minus size={15} /></button>
        <button aria-label="Развернуть" disabled={!isNative()} onClick={() => control(windowControls.toggleMaximize)}><Square size={12} /></button>
        <button className="close" aria-label="Закрыть" disabled={!isNative()} onClick={() => control(windowControls.close)}><X size={15} /></button>
      </div>
    </header>
  )
}

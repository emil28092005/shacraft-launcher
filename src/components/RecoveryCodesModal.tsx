import { useEffect, useRef } from 'react'

export function RecoveryCodesModal({ codes, onAcknowledge }: { codes: string[]; onAcknowledge: () => void }) {
  const button = useRef<HTMLButtonElement>(null)
  useEffect(() => {
    if (!codes.length) return
    const previous = document.activeElement
    button.current?.focus()
    return () => { if (previous instanceof HTMLElement) previous.focus() }
  }, [codes])
  if (!codes.length) return null
  return (
    <>
      <div className="drawer-backdrop visible" />
      <div className="login-modal" role="dialog" aria-modal="true" aria-labelledby="recovery-title"
        onKeyDown={(event) => { if (event.key === 'Tab') { event.preventDefault(); button.current?.focus() } }}>
        <h2 id="recovery-title">Коды восстановления</h2>
        <p>Сохраните их сейчас. Каждый код можно использовать один раз для восстановления пароля.</p>
        <div className="login-code recovery-codes">{codes.join('\n')}</div>
        <button ref={button} className="setting-row" onClick={onAcknowledge}><span>Я сохранил коды</span></button>
      </div>
    </>
  )
}

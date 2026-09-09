import { useEffect, useRef, useState } from 'react'
import { native } from '../services/native'
import { errorMessage } from '../services/async'
import type { LegacyMod } from '../types/launcher'

export function LegacyModsDialog({ profileId, onClose, onChanged }: { profileId: string; onClose: () => void; onChanged: () => void }) {
  const [files, setFiles] = useState<LegacyMod[]>([])
  const [selected, setSelected] = useState<Set<string>>(new Set())
  const [busy, setBusy] = useState(true)
  const [moving, setMoving] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [backup, setBackup] = useState<string | null>(null)
  const dialog = useRef<HTMLElement>(null)
  const closeButton = useRef<HTMLButtonElement>(null)
  const request = useRef(0)
  const moveInFlight = useRef(false)
  const mounted = useRef(false)

  useEffect(() => {
    mounted.current = true
    const previous = document.activeElement
    closeButton.current?.focus()
    return () => {
      mounted.current = false
      if (previous instanceof HTMLElement && previous.isConnected) previous.focus()
    }
  }, [])

  useEffect(() => {
    const current = ++request.current
    setFiles([]); setSelected(new Set()); setBackup(null); setError(null); setBusy(true)
    void native.legacyMods(profileId).then((value) => { if (request.current === current) setFiles(value) })
      .catch((reason) => { if (request.current === current) setError(errorMessage(reason, 'Не удалось проверить моды')) })
      .finally(() => { if (request.current === current) setBusy(false) })
    return () => { request.current++ }
  }, [profileId])

  const move = async () => {
    if (busy || moveInFlight.current || !selected.size) return
    const current = request.current
    moveInFlight.current = true
    setBusy(true); setMoving(true); setError(null)
    dialog.current?.focus()
    try {
      const result = await native.backupLegacyMods(profileId, files.filter((f) => selected.has(f.path)).map(({path,sha256}) => ({path,sha256})))
      moveInFlight.current = false
      if (mounted.current) setMoving(false)
      if (request.current !== current) return
      setBackup(result.backupRoot); setSelected(new Set())
      onChanged()
      const remaining = await native.legacyMods(profileId)
      if (request.current === current) setFiles(remaining)
    } catch (reason) {
      if (request.current === current) setError(errorMessage(reason, 'Не удалось перенести выбранные моды'))
    } finally {
      moveInFlight.current = false
      if (mounted.current) setMoving(false)
      if (request.current === current) setBusy(false)
    }
  }

  return <div className="legacy-overlay"><section ref={dialog} className="legacy-dialog" role="dialog" aria-modal="true"
    aria-labelledby="legacy-title" aria-describedby="legacy-description" tabIndex={-1}
    onKeyDown={(event) => {
      if (event.key === 'Escape') {
        event.preventDefault(); event.stopPropagation()
        if (!moveInFlight.current) onClose()
      }
      if (event.key !== 'Tab') return
      const elements = dialog.current?.querySelectorAll<HTMLElement>('button:not(:disabled), input:not(:disabled)')
      const first = elements?.[0]
      const last = elements?.[elements.length - 1]
      if (!first) { event.preventDefault(); dialog.current?.focus() }
      else if (event.shiftKey && (document.activeElement === first || document.activeElement === dialog.current)) {
        event.preventDefault(); last?.focus()
      } else if (!event.shiftKey && (document.activeElement === last || document.activeElement === dialog.current)) {
        event.preventDefault(); first.focus()
      }
    }}>
    <h2 id="legacy-title">Проверка старых и изменённых модов</h2>
    <p id="legacy-description">Эти файлы сохранены: их происхождение или содержимое отличается от ожидаемого. Здесь могут быть ваши моды. Выберите только те, которые хотите убрать из сборки в резервную копию. По умолчанию ничего не выбрано.</p>
    {busy && <p role="status">{moving ? 'Переносим выбранные файлы в резервную копию…' : 'Проверяем файлы…'}</p>}
    {!busy && files.length === 0 && <p>Файлов для ручного разбора нет.</p>}
    <div className="legacy-list">{files.map((file) => <label key={file.path}>
      <input type="checkbox" checked={selected.has(file.path)} disabled={busy} onChange={(e) => setSelected((old) => {
        const next = new Set(old); if (e.target.checked) next.add(file.path); else next.delete(file.path); return next
      })} />
      <span><strong>{file.path}</strong><small>{(file.size / 1024 / 1024).toFixed(1)} МБ · {file.reason === 'changed_managed' ? 'Изменён после установки' : 'Неизвестное происхождение'}</small>
        <small>SHA-256: {file.sha256}</small></span>
    </label>)}</div>
    {backup && <p role="status">Копии сохранены: {backup}. Теперь можно повторить проверку сборки.</p>}
    {error && <p className="status-error" role="alert">{error}</p>}
    <div className="legacy-actions"><button disabled={busy || selected.size === 0} onClick={() => { void move() }}>Перенести выбранные ({selected.size}) в резервную копию</button>
      <button ref={closeButton} disabled={moving} onClick={onClose}>Закрыть</button></div>
  </section></div>
}

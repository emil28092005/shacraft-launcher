import { Download, Gauge, Globe2, Play, RotateCcw, ShieldCheck, Wrench } from 'lucide-react'
import type { ProfileState } from '../state/profiles'
import { installPercent, installStageLabels } from '../state/game'
import type { GameOperation } from '../state/game'
import type { Server } from '../types/launcher'

interface PlayDockProps {
  server: Server
  operation: GameOperation
  profile: ProfileState | undefined
  memoryGb: number
  native: boolean
  needsLogin: boolean
  error: string | null
  label: string
  primaryDisabled: boolean
  repairDisabled: boolean
  onPrimary: () => void
  onRepair: () => void
}

export function PlayDock(props: PlayDockProps) {
  const { server, operation, profile, memoryGb, needsLogin, error } = props
  const progress = operation.phase === 'installing' ? operation.progress : null
  const percent = installPercent(progress)
  const working = operation.phase === 'syncing' || operation.phase === 'installing' || operation.phase === 'launching'
  let title: string
  let detail: string
  if (operation.phase === 'installing') {
    title = progress ? installStageLabels[progress.stage] : 'Готовим установку'
    detail = percent === null ? 'Проверяем файлы…' : `${percent}%`
  } else if (operation.phase === 'launching') {
    title = 'Запускаем игру'; detail = 'Подготавливаем игровой процесс…'
  } else if (operation.phase === 'running') {
    title = 'Игра запущена'; detail = 'Вернитесь после завершения игры'
  } else if (operation.phase === 'syncing') {
    title = 'Синхронизируем сборку'; detail = 'Скачиваем и проверяем файлы'
  } else if (server.disabled) {
    title = 'Техобслуживание'; detail = 'Сообщим, когда сервер вернётся'
  } else if (!props.native) {
    title = 'Предпросмотр интерфейса'; detail = 'Установка и запуск доступны в приложении'
  } else {
    title = needsLogin ? 'Нужен вход' : profile?.status === 'checking' ? 'Проверяем сборку'
      : profile?.inspection?.upToDate ? 'Сборка готова' : 'Требуется проверка'
    detail = profile?.inspection ? `${profile.inspection.managedFiles} файлов под контролем` : 'Проверяем локальные файлы'
  }

  return (
    <section className="play-dock">
      <div className="build-state" aria-live="polite">
        <span className={`state-icon ${working ? 'downloading' : server.disabled ? 'muted' : ''}`}>
          {working ? <Download size={19} /> : server.disabled ? <Wrench size={19} /> : <ShieldCheck size={19} />}
        </span>
        <span><strong>{title}</strong><small className={error ? 'status-error' : undefined} title={error ?? undefined}>{error ?? detail}</small></span>
        {percent !== null && <div className="progress-track" role="progressbar" aria-label={title}
          aria-valuemin={0} aria-valuemax={100} aria-valuenow={percent}><i style={{ width: `${percent}%` }} /></div>}
      </div>
      <div className="build-facts">
        <span><Globe2 size={15} /> {server.version}</span>
        <span><Gauge size={15} /> {memoryGb} ГБ памяти</span>
      </div>
      <button className="repair-button" onClick={props.onRepair} disabled={props.repairDisabled} aria-label="Проверить файлы"><RotateCcw size={19} /></button>
      <button className="play-button" disabled={props.primaryDisabled} onClick={props.onPrimary}>
        <Play size={21} fill="currentColor" /><span>{props.label}</span>
      </button>
    </section>
  )
}

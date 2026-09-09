import type { ReactNode } from 'react'
import { Users } from 'lucide-react'
import { isNative } from '../services/native'
import type { Server, ServerStatus } from '../types/launcher'

export function ServerStage({ server, status, children }: { server: Server; status: ServerStatus | null; children: ReactNode }) {
  const online = status?.reachable && status.online !== null && status.max !== null
    ? `${status.online} / ${status.max}` : !isNative() ? 'В приложении' : status === null ? 'Проверяем…' : 'Нет связи'
  const label = !isNative() ? 'Статус в приложении' : status === null ? 'Проверяем сервер' : status.reachable ? 'Сервер доступен' : 'Сервер недоступен'
  return (
    <main className={`stage stage-${server.id}`}>
      <div className="stage-top">
        <div className={`live-pill ${status?.reachable ? '' : 'offline'}`}><span /> {label}</div>
        <div className="players"><Users size={16} /> {online}</div>
      </div>
      <section className="hero-copy">
        <p>{server.kicker}</p><h1>{server.name}</h1><h2>{server.subtitle}</h2>
        <dl className="hero-meta">
          <div><dt>Загрузчик</dt><dd>{server.loader}</dd></div>
          <div><dt>Java</dt><dd>Версия 21</dd></div>
        </dl>
      </section>
      {children}
    </main>
  )
}

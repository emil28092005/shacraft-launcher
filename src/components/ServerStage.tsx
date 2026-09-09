import type { ReactNode } from 'react'
import { Users } from 'lucide-react'
import type { Server } from '../types/launcher'

export function ServerStage({ server, children }: { server: Server; children: ReactNode }) {
  return (
    <main className={`stage stage-${server.id}`}>
      <div className="stage-top">
        <div className="live-pill offline"><span /> {server.disabled ? 'Не в сети' : 'Статус не проверен'}</div>
        <div className="players"><Users size={16} /> {server.disabled ? 'Сервер остановлен' : 'Онлайн неизвестен'}</div>
      </div>
      <section className="hero-copy">
        <p>{server.kicker}</p><h1>{server.name}</h1><h2>{server.subtitle}</h2>
        <dl className="hero-meta">
          <div><dt>Состав</dt><dd>{server.composition}</dd></div>
          <div><dt>Загрузчик</dt><dd>{server.loader}</dd></div>
          <div><dt>Java</dt><dd>Версия 21</dd></div>
        </dl>
      </section>
      {children}
    </main>
  )
}

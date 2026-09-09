import type { DeviceCodePayload } from '../types/launcher'

export function LoginModal({ code }: { code: DeviceCodePayload | null }) {
  if (!code) return null
  return (
    <>
      <div className="drawer-backdrop visible" />
      <div className="login-modal" role="dialog" aria-modal="true" aria-labelledby="login-title">
        <h2 id="login-title">Вход через Microsoft</h2>
        <p>Откройте страницу и введите код, чтобы подтвердить вход в аккаунт с лицензией Minecraft.</p>
        <div className="login-code">{code.userCode}</div>
        <p className="login-url">{code.verificationUri}</p>
      </div>
    </>
  )
}

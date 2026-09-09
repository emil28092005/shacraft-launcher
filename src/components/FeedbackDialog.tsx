import { useEffect, useRef } from 'react'

export interface Feedback {
  kind: 'info' | 'success' | 'error'
  title: string
  message: string
}

export function FeedbackDialog({ feedback, onDismiss }: { feedback: Feedback | null; onDismiss: () => void }) {
  const dialog = useRef<HTMLDialogElement>(null)
  const visible = feedback !== null
  useEffect(() => {
    const element = dialog.current
    if (!visible || !element) return
    const previous = document.activeElement
    element.showModal()
    return () => {
      element.close()
      if (previous instanceof HTMLElement && previous.isConnected) previous.focus()
    }
  }, [visible])
  if (!feedback) return null
  return (
    <dialog ref={dialog} className={`feedback-dialog ${feedback.kind}`} aria-labelledby="feedback-title"
      aria-describedby="feedback-message" onCancel={(event) => { event.preventDefault(); onDismiss() }}
      onKeyDown={(event) => event.stopPropagation()}>
      <div aria-live={feedback.kind === 'error' ? 'assertive' : 'polite'} aria-atomic="true">
        <h2 id="feedback-title">{feedback.title}</h2>
        <p id="feedback-message">{feedback.message}</p>
      </div>
      <button type="button" autoFocus onClick={onDismiss}>Понятно</button>
    </dialog>
  )
}

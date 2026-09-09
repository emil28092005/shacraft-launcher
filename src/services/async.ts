export function errorMessage(error: unknown, fallback: string): string {
  if (error instanceof Error && error.message) return error.message
  // Tauri rejects commands with the Rust error string, not an Error instance.
  if (typeof error === 'string' && error.trim()) return error
  return fallback
}

/** Share a pending restore across React StrictMode's effect restart. */
export function singleFlight<T>(operation: () => Promise<T>): () => Promise<T> {
  let pending: Promise<T> | null = null
  return () => {
    if (pending) return pending
    const request = Promise.resolve().then(operation)
    pending = request
    const clear = () => { if (pending === request) pending = null }
    void request.then(clear, clear)
    return request
  }
}

/** Serializes writes so a slow older save cannot overwrite a newer choice. */
export function createSerialQueue() {
  let tail: Promise<unknown> = Promise.resolve()
  return {
    enqueue<T>(operation: () => Promise<T>): Promise<T> {
      const result = tail.then(operation)
      // A failed write must not poison all subsequent retries.
      tail = result.catch(() => undefined)
      return result
    },
    settled: () => tail,
  }
}

/** Handles unmount before asynchronous native listener registration finishes. */
export function createSubscription(
  registrations: readonly Promise<() => void>[],
) {
  let disposed = false
  const cleanups = new Set<() => void>()
  const dispose = () => {
    disposed = true
    cleanups.forEach((cleanup) => cleanup())
    cleanups.clear()
  }
  const ready = Promise.all(registrations.map(async (registration) => {
    const cleanup = await registration
    if (disposed) cleanup()
    else cleanups.add(cleanup)
  })).then(() => undefined).catch((error: unknown) => {
    dispose()
    throw error
  })
  return { ready, dispose }
}

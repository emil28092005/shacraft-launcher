import { equal } from 'node:assert/strict'
import { test } from 'node:test'
import { profilesReducer } from './profiles.ts'

test('a failed repair invalidates an old ready inspection and permits retry', () => {
  const profileId = 'aeronautics'
  const inspection = { root: '/profiles/aeronautics', managedFiles: 251, missingFiles: 0, mismatchedFiles: 0, upToDate: true }
  const ready = profilesReducer({}, { type: 'checked', profileId, inspection })
  equal(ready[profileId]?.inspection?.upToDate, true)
  const repairing = profilesReducer(ready, { type: 'check', profileId })
  equal(repairing[profileId]?.inspection, null)
  const failed = profilesReducer(repairing, { type: 'failed', profileId, error: 'Download interrupted' })
  equal(failed[profileId]?.status, 'error')
  equal(failed[profileId]?.inspection, null)
  const retrying = profilesReducer(failed, { type: 'check', profileId })
  const repaired = profilesReducer(retrying, { type: 'checked', profileId, inspection })
  equal(repaired[profileId]?.inspection?.upToDate, true)
  equal(repaired[profileId]?.error, null)
})

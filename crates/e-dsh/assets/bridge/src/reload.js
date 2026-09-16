import { shapeSkillsFrame } from './skill.js'
import { shapeCommandsFrame } from './command.js'

export function createResourceReload({ host }) {
  let registration
  return async (conn, signal) => {
    const skills = host.skills()
    if (!skills?.registerProvider || !skills?.snapshot) throw new Error('DSH skill reload API unavailable')
    if (!registration || registration.signal?.aborted) {
      // The registry exposes invalidation through an owned provider registration,
      // not through list(). This empty provider contributes no skills.
      skills.registerProvider(control => {
        registration = control
        return { name: 'e-resource-reload', list: async () => [], get: async () => undefined }
      })
    }
    if (!registration?.invalidate) {
      throw new Error('DSH skill registry did not expose provider invalidation synchronously')
    }
    registration.invalidate()
    const snapshot = await skills.snapshot({
      cwd: conn.agent.session?.header?.cwd,
      scope: conn.agent,
      signal,
    })
    signal.throwIfAborted()
    if (!snapshot.complete) throw new Error('DSH skill discovery incomplete; retry /reload')
    const commands = host.commands()
    if (!commands) throw new Error('DSH command directory unavailable')
    return [shapeSkillsFrame(snapshot.skills), shapeCommandsFrame(commands.list(conn.agent))]
  }
}

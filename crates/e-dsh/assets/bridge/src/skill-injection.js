import { renderSkillContent, skillInvocationSource } from './skill.js'

export function createSkillInjector({ host, conns, send, createUserMessage }) {
  return async function injectSkill(ws, current, name, prompt = '') {
    if (!current) return
    const skills = host.skills()
    if (!skills) {
      send(ws, { type: 'error', code: 'skill-unavailable', message: 'skills service unavailable' })
      return
    }
    try {
      const skill = await skills.get(name, {
        cwd: current.agent.session?.header?.cwd,
        signal: current.abort.signal,
        scope: current.agent,
      })
      if (!conns.has(current)) return
      if (!skill) {
        send(ws, { type: 'error', code: 'skill-unknown', message: `skill "${name}" is unknown or no longer available` })
        return
      }
      current.agent.followup(createUserMessage({
        content: [{ type: 'text', text: renderSkillContent(skill) }],
        source: skillInvocationSource(name),
      }))
      if (prompt.trim() !== '') {
        current.agent.followup(createUserMessage({
          content: [{ type: 'text', text: prompt }],
          source: { kind: 'user' },
        }))
      }
    } catch (error) {
      if (!conns.has(current)) return
      send(ws, { type: 'error', code: 'skill-failed', message: String(error?.message ?? error) })
    }
  }
}

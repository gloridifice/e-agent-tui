// /skill:<name> layer: skill-name grammar, the `/skill` command parser, and
// the model-facing `<skill_content>` renderer. These are pure functions
// (test/skill.test.js) — the DSH `skills` service does the actual discovery
// from `~/.agents/skills/` and `<workspace>/.agents/skills/` (project wins
// over user by rank); the bridge only looks a name up and injects the
// rendered content, mirroring dsh-tool-skill's user-explicit invocation.

/** Public skill-name grammar (kebab-case, matches @deepseek-ai/dsh-skill). */
export const SKILL_NAME = /^[a-z0-9]+(?:-[a-z0-9]+)*$/

export function isSkillName(name) {
  return typeof name === 'string' && SKILL_NAME.test(name)
}

/** Shape the user-invocable part of `ctx.skills.list()` for TUI completion. */
export function shapeSkillsFrame(summaries) {
  const skills = []
  for (const summary of Array.isArray(summaries) ? summaries : []) {
    if (!summary || !isSkillName(summary.name)) continue
    if (typeof summary.description !== 'string') continue
    if (summary.invocation?.userInvocable !== true) continue
    skills.push({ name: summary.name, description: summary.description })
  }
  skills.sort((left, right) => left.name.localeCompare(right.name))
  return { type: 'skills', skills }
}

/** `skills/change` is an unfiltered invalidation; every connection must
 * refetch its cwd/scope-sensitive winning roster. */
export function watchSkillChanges(ctx, connections, refresh) {
  return ctx.on('skills/change', () => {
    for (const connection of connections) refresh(connection)
  })
}

/**
 * Extract the skill name from a `/skill:<name>` or `/skill <name>` command
 * line, or undefined when the line is not such a command. The colon form is
 * the documented syntax; the space form is accepted as a convenience.
 */
export function parseSkillCommand(line) {
  if (typeof line !== 'string') return undefined
  const trimmed = line.trim()
  const colon = trimmed.match(/^\/skill:([a-z0-9]+(?:-[a-z0-9]+)*)\s*$/i)
  if (colon) return colon[1].toLowerCase()
  const space = trimmed.match(/^\/skill\s+([a-z0-9]+(?:-[a-z0-9]+)*)\s*$/i)
  if (space) return space[1].toLowerCase()
  return undefined
}

function escapeAttr(value) {
  return value.replaceAll('&', '&amp;').replaceAll('"', '&quot;').replaceAll('<', '&lt;')
}

function escapeText(value) {
  return value.replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;')
}

function resourceHint(skill) {
  const base = skill.resourceBase
  if (base === undefined) {
    return [
      `Resources for this skill are managed by provider "${escapeText(skill.provider)}".`,
      'Load referenced resources only as needed.',
    ]
  }
  switch (base.kind) {
    case 'directory':
      return [
        `Base directory for this skill: ${escapeText(base.path)}`,
        'Resolve relative paths mentioned by this skill against the base directory before using them. Load referenced resources only as needed.',
      ]
    case 'url':
      return [
        `Base URL for this skill: ${escapeText(base.url)}`,
        'Resolve relative URLs mentioned by this skill against the base URL before using them. Load referenced resources only as needed.',
      ]
    case 'opaque':
      return [
        `Resources for this skill: ${escapeText(base.description)}`,
        'Load referenced resources only as needed.',
      ]
    default:
      throw new Error(`SkillResourceBase.kind: ${base.kind}`)
  }
}

/**
 * Render one loaded skill for the model — the same canonical
 * `<skill_content>` shape dsh-tool-skill emits, so a user-explicit `/skill`
 * injection reads exactly like the model's own `skill` tool result.
 */
export function renderSkillContent(skill) {
  return [
    `<skill_content name="${escapeAttr(skill.name)}">`,
    '<skill_resources>',
    ...resourceHint(skill),
    '</skill_resources>',
    '',
    '<skill_instructions>',
    skill.content,
    '</skill_instructions>',
    '</skill_content>',
  ].join('\n')
}

/**
 * Build the `source` record for an injected skill invocation, matching
 * dsh-tool-skill's `source: { kind: "skill-invocation", name, form:
 * "instructions" }`.
 */
export function skillInvocationSource(name) {
  return { kind: 'skill-invocation', name, form: 'instructions' }
}

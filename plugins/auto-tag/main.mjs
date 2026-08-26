/**
 * Auto-tagging hook.
 *
 * Shows the *other* half of the plugin surface: reacting to lifecycle hooks and
 * returning a patch that the host merges into the item before it is persisted.
 *
 * Deliberately rule-based rather than model-based — it must never be the reason
 * a save is slow, and its behaviour has to be predictable and explainable.
 */
import { createInterface } from 'node:readline'

/** [tag, matcher] pairs. Order does not matter; every match applies. */
const RULES = [
  ['survey', /\b(surveys?|reviews?|systematic review)\b|综述|述评/i],
  ['benchmark', /\bbenchmarks?\b|评测基准/i],
  ['dataset', /\bdatasets?\b|数据集/i],
  ['theory', /\b(theorems?|proofs?|convergence|bounds?)\b|定理|收敛性/i],
  ['llm', /\b(large language models?|llms?|gpt|transformers?)\b|大模型|大语言模型/i],
  ['diffusion', /\bdiffusion (models?|probabilistic)\b|扩散模型/i],
  ['rl', /\breinforcement learning\b|强化学习/i],
  ['vision', /\b(computer vision|image (classification|segmentation))\b|计算机视觉/i],
  ['chinese', /[\u4e00-\u9fff]/],
]

function send(message) {
  process.stdout.write(JSON.stringify(message) + '\n')
}

function tagsFor(item) {
  const haystack = [item?.fields?.title, item?.fields?.abstractNote, ...(item?.creators ?? [])]
    .filter(Boolean)
    .join(' ')
  if (!haystack) return []
  return RULES.filter(([, re]) => re.test(haystack)).map(([tag]) => tag)
}

const methods = {
  initialize: () => ({ contributions: {} }),

  hook: ({ name, payload } = {}) => {
    if (name !== 'item.beforeCreate') return {}
    // The host sends the whole batch and expects patches aligned by position;
    // one round-trip per item would hold its write lock for the whole import.
    const patches = (payload?.items ?? []).map((item) => {
      const tags = tagsFor(item)
      // The host merges `tags` as automatic tags and `fields` only where absent.
      return tags.length ? { tags } : null
    })
    return { patches }
  },

  shutdown: () => null,
}

createInterface({ input: process.stdin }).on('line', (line) => {
  if (!line.trim()) return
  let message
  try {
    message = JSON.parse(line)
  } catch {
    return
  }
  const handler = methods[message.method]
  if (!handler) {
    send({
      jsonrpc: '2.0',
      id: message.id,
      error: { code: -32601, message: `unknown method '${message.method}'` },
    })
    return
  }
  try {
    send({ jsonrpc: '2.0', id: message.id, result: handler(message.params ?? {}) ?? null })
    if (message.method === 'shutdown') process.exit(0)
  } catch (err) {
    send({
      jsonrpc: '2.0',
      id: message.id,
      error: { code: -32000, message: String(err?.message ?? err) },
    })
  }
}).on('close', () => process.exit(0))

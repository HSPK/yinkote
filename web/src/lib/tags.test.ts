import { describe, expect, it } from 'vitest'

import { hasChosenColour, tagColour, TAG_COLOURS } from './tags'

describe('tag colour', () => {
  it('gives every tag a colour without anybody assigning one', () => {
    // Grey until somebody colours two hundred tags by hand means nobody ever
    // has colours, because that afternoon never comes.
    expect(TAG_COLOURS).toContain(tagColour('transformer'))
    expect(TAG_COLOURS).toContain(tagColour('扩散模型'))
  })

  it('gives the same tag the same colour every time', () => {
    // The eye learns a colour. One that changes between renders is worse than
    // none, because it then misleads.
    expect(tagColour('survey')).toBe(tagColour('survey'))
    expect(tagColour('survey')).not.toBe(tagColour('survey '))
  })

  it('lets an explicit choice win', () => {
    expect(tagColour('survey', 'red')).toBe('red')
  })

  it('ignores a colour it cannot draw rather than honouring it', () => {
    // A library written by a newer version may name a colour this build has
    // never heard of, and a tag is not worth losing over that.
    expect(TAG_COLOURS).toContain(tagColour('survey', 'octarine'))
    expect(tagColour('survey', 'octarine')).toBe(tagColour('survey'))
  })

  it('knows whether a colour was chosen or derived', () => {
    expect(hasChosenColour('blue')).toBe(true)
    expect(hasChosenColour(null)).toBe(false)
    expect(hasChosenColour('octarine')).toBe(false)
  })

  it('spreads names across the palette rather than piling them up', () => {
    const names = Array.from({ length: 200 }, (_, i) => `tag-${i}`)
    const used = new Set(names.map((n) => tagColour(n)))
    // A hash that lands everything on two colours is not colouring anything.
    expect(used.size).toBe(TAG_COLOURS.length)
  })

  it('treats a CJK tag the same as an English one', () => {
    // Hashing bytes rather than code points makes CJK names collide through a
    // lossy conversion; hashing code points does not.
    const cjk = ['综述', '注意力机制', '扩散模型', '知识图谱', '强化学习', '多模态']
    expect(new Set(cjk.map((n) => tagColour(n))).size).toBeGreaterThan(1)
  })
})

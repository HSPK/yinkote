import { afterEach, beforeEach, expect, it, vi } from 'vitest'
import { createRoot, type Root } from 'react-dom/client'
import { act } from 'react'

import { KeyGate } from './KeyGate'
import { useStore } from '../state/store'

let container: HTMLElement
let root: Root

beforeEach(() => {
  container = document.createElement('div')
  document.body.append(container)
  root = createRoot(container)
})
afterEach(() => {
  act(() => root.unmount())
  container.remove()
})

it('asks for the key and says where it is kept', () => {
  // The state this replaced was the word "connecting", for ever, which reads
  // as a broken server rather than as a library asking who you are.
  act(() => root.render(<KeyGate />))
  const text = container.textContent ?? ''
  expect(text).toContain('needs a key')
  expect(text).toContain('YK_API_KEY')
  // The key is typed, never displayed.
  expect(container.querySelector('input')?.getAttribute('type')).toBe('password')
})

it('will not submit an empty key', () => {
  act(() => root.render(<KeyGate />))
  expect(container.querySelector('button')?.hasAttribute('disabled')).toBe(true)
})

it('hands the typed key to the store', async () => {
  const used: string[] = []
  useStore.setState({ useApiKey: async (k: string) => void used.push(k) })

  act(() => root.render(<KeyGate />))
  const input = container.querySelector('input') as HTMLInputElement
  const setter = Object.getOwnPropertyDescriptor(window.HTMLInputElement.prototype, 'value')!.set!
  act(() => {
    setter.call(input, 'topsecret')
    input.dispatchEvent(new Event('input', { bubbles: true }))
  })
  await act(async () => {
    container.querySelector('form')!.dispatchEvent(new Event('submit', { bubbles: true, cancelable: true }))
  })
  expect(used).toEqual(['topsecret'])
})

import { describe, expect, it } from 'vitest'
import { ApiError, unwrapEnvelope } from '../shared/envelope.js'

describe('api envelope', () => {
  it('unwraps success data and throws business errors', () => {
    expect(unwrapEnvelope({ code: 0, message: 'ok', data: { value: 1 } })).toEqual({ value: 1 })
    expect(() => unwrapEnvelope({ code: 200001, message: '未登录', data: null })).toThrow(ApiError)
  })
})

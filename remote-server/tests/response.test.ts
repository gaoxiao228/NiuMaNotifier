import { describe, expect, it } from 'vitest'
import { ErrorCode } from '../src/shared/errors.js'
import { apiFailure, apiSuccess } from '../src/shared/response.js'

describe('standard API envelope', () => {
  it('returns success envelope with object data', () => {
    expect(apiSuccess({ service: 'remote' })).toEqual({
      code: 0,
      message: 'ok',
      data: { service: 'remote' }
    })
  })

  it('returns failure envelope with outer code and message', () => {
    expect(apiFailure(ErrorCode.UNAUTHORIZED, '未登录')).toEqual({
      code: 200001,
      message: '未登录',
      data: null
    })
  })
})

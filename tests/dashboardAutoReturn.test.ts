import { shouldReturnToDashboardForState } from '../src/dashboardAutoReturn'
import type { MainStatePayload } from '../src/api'

function state(status: string, eventId: string | null = null): MainStatePayload {
  return {
    version: 1,
    status,
    updated_at: '2026-06-22T10:00:00Z',
    session: null,
    detail: eventId
      ? {
          event_id: eventId,
          event_type: status === 'waiting_approval' ? 'approval_requested' : status,
          severity: 'urgent',
          summary: '需要处理',
          content: '阻塞请求',
          error_message: null,
          payload_ref: null,
          completion_reason: null,
          failure_reason: null,
          approval: null
        }
      : null
  }
}

if (
  !shouldReturnToDashboardForState({
    activeView: 'settings',
    previousState: state('running', 'event-running'),
    nextState: state('waiting_approval', 'event-approval-1')
  })
) {
  throw new Error('从非阻塞状态进入授权等待时应自动回到首页')
}

if (
  shouldReturnToDashboardForState({
    activeView: 'settings',
    previousState: state('waiting_approval', 'event-approval-1'),
    nextState: state('waiting_approval', 'event-approval-1')
  })
) {
  throw new Error('同一个阻塞状态重复刷新时不应反复回到首页')
}

if (
  !shouldReturnToDashboardForState({
    activeView: 'settings',
    previousState: state('waiting_approval', 'event-approval-1'),
    nextState: state('waiting_input', 'event-input-1')
  })
) {
  throw new Error('新的阻塞事件出现时应自动回到首页')
}

if (
  shouldReturnToDashboardForState({
    activeView: 'dashboard',
    previousState: state('running', 'event-running'),
    nextState: state('error', 'event-error-1')
  })
) {
  throw new Error('已经在首页时不需要重复切换首页')
}

import { isBlockingStatus } from './statusView'
import type { MainStatePayload } from './api'

export type DashboardAutoReturnView = 'dashboard' | 'settings'

export type DashboardAutoReturnInput = {
  activeView: DashboardAutoReturnView
  previousState: MainStatePayload | null
  nextState: MainStatePayload
}

export function shouldReturnToDashboardForState(input: DashboardAutoReturnInput) {
  if (input.activeView === 'dashboard' || !isBlockingStatus(input.nextState.status)) {
    return false
  }
  if (!input.previousState || !isBlockingStatus(input.previousState.status)) {
    return true
  }
  // 同一个阻塞项重复刷新不打断用户；只有新的阻塞项或阻塞类型变化时才回首页。
  return (
    input.previousState.status !== input.nextState.status ||
    input.previousState.detail?.event_id !== input.nextState.detail?.event_id
  )
}

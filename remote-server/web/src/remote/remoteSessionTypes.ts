export type RemoteSessionProjectGroupPage = {
  list: RemoteSessionProjectGroup[]
  page?: number
  page_size?: number
  total?: number
}

export type RemoteSessionProjectGroup = {
  tool?: string
  project_name?: string
  project_path?: string
  sessions: RemoteSessionSummary[]
}

export type RemoteSessionSummary = {
  normalized_session_id?: string
  primary_session_id?: string
  title?: string
  status?: string
  runtime_status?: string | null
  updated_at?: string
  first_user_message_preview?: string
  latest_event_summary?: string | null
  subagent_count?: number
}

export function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === 'object' && value !== null
}

export function isRemoteSessionSummary(value: unknown): value is RemoteSessionSummary {
  return (
    isRecord(value) &&
    (typeof value.normalized_session_id === 'undefined' || typeof value.normalized_session_id === 'string') &&
    (typeof value.primary_session_id === 'undefined' || typeof value.primary_session_id === 'string') &&
    (typeof value.title === 'undefined' || typeof value.title === 'string') &&
    (typeof value.status === 'undefined' || typeof value.status === 'string') &&
    (typeof value.runtime_status === 'undefined' ||
      value.runtime_status === null ||
      typeof value.runtime_status === 'string') &&
    (typeof value.updated_at === 'undefined' || typeof value.updated_at === 'string') &&
    (typeof value.first_user_message_preview === 'undefined' || typeof value.first_user_message_preview === 'string') &&
    (typeof value.latest_event_summary === 'undefined' ||
      value.latest_event_summary === null ||
      typeof value.latest_event_summary === 'string') &&
    (typeof value.subagent_count === 'undefined' || typeof value.subagent_count === 'number')
  )
}

export function isProjectGroupPage(value: unknown): value is RemoteSessionProjectGroupPage {
  // RPC 返回值是 unknown，这里只校验渲染所需的最小结构。
  return (
    isRecord(value) &&
    Array.isArray(value.list) &&
    value.list.every(
      (group) =>
        isRecord(group) &&
        (typeof group.tool === 'undefined' || typeof group.tool === 'string') &&
        (typeof group.project_name === 'undefined' || typeof group.project_name === 'string') &&
        (typeof group.project_path === 'undefined' || typeof group.project_path === 'string') &&
        Array.isArray(group.sessions) &&
        group.sessions.every(isRemoteSessionSummary)
    )
  )
}

export function sessionDisplayStatus(session: RemoteSessionSummary): string | null {
  return session.runtime_status || session.status || null
}

export function sessionTitle(session: RemoteSessionSummary): string {
  return session.title || session.primary_session_id || session.normalized_session_id || ''
}

export function sessionDescription(session: RemoteSessionSummary): string | null {
  return session.first_user_message_preview || session.latest_event_summary || session.primary_session_id || null
}

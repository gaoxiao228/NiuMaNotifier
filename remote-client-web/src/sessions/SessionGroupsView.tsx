import { Alert, Empty, List, Skeleton, Space, Tag } from 'antd'

import { useI18n } from '../i18n/index.js'
import {
  sessionDescription,
  sessionDisplayStatus,
  sessionTitle,
  type RemoteSessionProjectGroup,
  type RemoteSessionProjectGroupPage,
  type RemoteSessionSummary
} from '../remote/remoteSessionTypes.js'

type SessionGroupsViewProps = {
  page: RemoteSessionProjectGroupPage | null
  loading: boolean
  error: string | null
}

type SessionListItem = {
  group: RemoteSessionProjectGroup
  session: RemoteSessionSummary
}

function formatUpdatedAt(value: string | undefined, fallback: string): string {
  if (!value) return fallback
  const date = new Date(value)
  if (Number.isNaN(date.getTime())) return fallback
  return date.toLocaleString()
}

function sessionKey(item: SessionListItem, index: number): string {
  return (
    item.session.normalized_session_id ||
    item.session.primary_session_id ||
    `${item.group.project_path ?? item.group.project_name ?? 'project'}-${index}`
  )
}

function flattenSessions(page: RemoteSessionProjectGroupPage | null): SessionListItem[] {
  if (!page) return []
  return page.list.flatMap((group) => group.sessions.map((session) => ({ group, session })))
}

function statusColor(status: string | null): string {
  switch (status) {
    case 'running':
      return 'processing'
    case 'waiting_input':
    case 'waiting_approval':
      return 'warning'
    case 'error':
      return 'error'
    case 'completed':
      return 'success'
    default:
      return 'default'
  }
}

function localizedStatus(t: ReturnType<typeof useI18n>['t'], status: string | null): string {
  switch (status) {
    case 'running':
      return t('session_status_running')
    case 'waiting_input':
      return t('session_status_waiting_input')
    case 'waiting_approval':
      return t('session_status_waiting_approval')
    case 'error':
      return t('session_status_error')
    case 'completed':
      return t('session_status_completed')
    default:
      return status || t('session_status_unknown')
  }
}

export function SessionGroupsView({ page, loading, error }: SessionGroupsViewProps) {
  const { t } = useI18n()
  const sessions = flattenSessions(page)

  if (loading) {
    return (
      <section className="session-list-panel" aria-label={t('session_list_title')}>
        <Skeleton active paragraph={{ rows: 6 }} />
      </section>
    )
  }

  if (error) {
    return (
      <section className="session-list-panel" aria-label={t('session_list_title')}>
        <Alert type="error" showIcon message={t('session_list_error')} description={error} />
      </section>
    )
  }

  return (
    <section className="session-list-panel" aria-label={t('session_list_title')}>
      <List
        className="session-groups-list"
        dataSource={sessions}
        locale={{ emptyText: <Empty description={t('session_list_empty')} /> }}
        renderItem={(item, index) => {
          const status = sessionDisplayStatus(item.session)
          const title = sessionTitle(item.session) || t('session_title_fallback')
          const projectPath = item.group.project_path || item.group.project_name || t('session_project_unknown')
          const description = sessionDescription(item.session)

          return (
            <List.Item key={sessionKey(item, index)} className="session-row">
              <List.Item.Meta
                title={
                  <div className="session-row-title">
                    <span>{title}</span>
                    <Tag color={statusColor(status)}>{localizedStatus(t, status)}</Tag>
                  </div>
                }
                description={
                  <div className="session-row-body">
                    {description ? <p>{description}</p> : null}
                    <dl className="session-row-meta">
                      <div>
                        <dt>{t('session_project_path_label')}</dt>
                        <dd>{projectPath}</dd>
                      </div>
                      <div>
                        <dt>{t('session_tool_label')}</dt>
                        <dd>{item.group.tool || t('session_tool_unknown')}</dd>
                      </div>
                      <div>
                        <dt>{t('session_updated_at_label')}</dt>
                        <dd>{formatUpdatedAt(item.session.updated_at, t('session_updated_at_unknown'))}</dd>
                      </div>
                    </dl>
                  </div>
                }
              />
            </List.Item>
          )
        }}
      />
      <Space className="session-list-summary">
        <span>{t('session_list_count')}</span>
        <strong>{sessions.length}</strong>
      </Space>
    </section>
  )
}

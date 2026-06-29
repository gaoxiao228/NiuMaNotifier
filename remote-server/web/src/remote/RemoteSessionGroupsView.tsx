import {
  sessionDescription,
  sessionDisplayStatus,
  sessionTitle,
  type RemoteSessionProjectGroupPage
} from './remoteSessionTypes.js'

type RemoteSessionGroupsViewProps = {
  page: RemoteSessionProjectGroupPage
  emptyText: string
}

export function RemoteSessionGroupsView({ page, emptyText }: RemoteSessionGroupsViewProps) {
  const groups = page.list
  if (groups.length === 0 || groups.every((group) => group.sessions.length === 0)) {
    return <p className="state-message">{emptyText}</p>
  }

  return (
    <div className="remote-session-groups">
      {groups.map((group, groupIndex) => (
        <div className="remote-session-group" key={`${group.project_path ?? group.project_name ?? groupIndex}`}>
          <div className="remote-session-group-heading">
            {group.project_name ? <strong>{group.project_name}</strong> : null}
            {group.project_path ? <span>{group.project_path}</span> : null}
            {group.tool ? <span>{group.tool}</span> : null}
          </div>
          <div className="remote-session-list">
            {group.sessions.map((session, sessionIndex) => {
              const displayStatus = sessionDisplayStatus(session)
              const description = sessionDescription(session)
              return (
                <div
                  className="remote-session-row"
                  key={session.normalized_session_id ?? session.primary_session_id ?? `${groupIndex}-${sessionIndex}`}
                >
                  <div className="remote-session-main">
                    <strong>{sessionTitle(session)}</strong>
                    {description ? <span>{description}</span> : null}
                  </div>
                  {displayStatus ? <span className="remote-session-status">{displayStatus}</span> : null}
                </div>
              )
            })}
          </div>
        </div>
      ))}
    </div>
  )
}

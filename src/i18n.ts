export type LanguageCode = 'zh-CN' | 'zh-TW' | 'en' | 'ja' | 'ko' | 'de'

export type Translation = {
  languageName: string
  appSubtitle: string
  mainStatus: string
  statusLegend: string
  needsHandling: string
  settings: string
  statusTab: string
  notificationTab: string
  notificationPlugins: string
  notificationSettings: string
  barkSettings: string
  ntfySettings: string
  enabled: string
  disabled: string
  server: string
  deviceKey: string
  group: string
  topic: string
  token: string
  save: string
  saved: string
  saving: string
  testSend: string
  sending: string
  sent: string
  lastResult: string
  notificationRules: string
  notificationHistory: string
  notificationHealth: string
  notificationChannelsReady: string
  latestNotification: string
  noChannelsEnabled: string
  listenerStatus: string
  localSseInterface: string
  localSseState: string
  localSsePort: string
  localSsePath: string
  localSseUrl: string
  localSseConnected: string
  localSsePolling: string
  expandNotificationSettings: string
  collapseNotificationSettings: string
  noNotificationRecords: string
  notificationChannel: string
  notificationTitle: string
  notificationContent: string
  notificationReason: string
  notificationCreatedAt: string
  notificationSentAt: string
  notificationError: string
  notifyApproval: string
  notifyInput: string
  notifyFailure: string
  notifyCompletion: string
  codexListening: string
  codexListenerDescription: string
  toolListenerDescription: string
  codexListeningOn: string
  codexListeningOff: string
  listenerSaving: string
  settingsButton: string
  backToDashboard: string
  pluginManagement: string
  pluginManagementDescription: string
  pluginConfig: string
  importPlugin: string
  importingPlugin: string
  pluginImportCancelled: string
  pluginImportSuccess: string
  removePlugin: string
  pluginRemoveSuccess: string
  noPlugins: string
  pluginSource: string
  pluginVersion: string
  pluginRuntimeStatus: string
  pluginInstallPath: string
  pluginLastError: string
  pluginCapabilities: string
  pluginCapabilityEventWatcher: string
  pluginCapabilityEventConsumer: string
  pluginCapabilityApprovalHandler: string
  pluginCapabilityNotificationTest: string
  pluginCapabilityStateConsumer: string
  pluginCapabilityToolSessionListProvider: string
  pluginCapabilityToolSessionDetailProvider: string
  pluginCapabilityToolSessionListReader: string
  pluginCapabilityToolSessionDetailReader: string
  pluginStarting: string
  pluginRunning: string
  pluginStopped: string
  pluginStopping: string
  pluginFailed: string
  pluginBuiltin: string
  pluginExternal: string
  language: string
  refresh: string
  clearBlocker: string
  clearBlockerAfterTool: string
  clearBlockerConfirm: string
  clearBlockerConfirmAgain: string
  clearBlockerClearing: string
  approveApproval: string
  denyApproval: string
  approvalSubmitting: string
  submitInputAnswer: string
  customInputAnswer: string
  inputTextPlaceholder: string
  // 会话详情控制区文案由 sessionWorkbenchView 通过 text 注入使用。
  sessionControlPlaceholder: string
  sessionControlSend: string
  sessionControlInterrupt: string
  sessionControlUnsupported: string
  sessionControlFailed: string
  currentRequest: string
  handlingHint: string
  project: string
  path: string
  toolLabel: string
  requestContent: string
  requestTime: string
  currentStatus: string
  activeSession: string
  sessionList: string
  sessionOverview: string
  projectName: string
  sessionId: string
  lastActivity: string
  latestEvent: string
  recentEvents: string
  noSessions: string
  noSessionSelected: string
  none: string
  noEvents: string
  eventCenter: string
  eventCenterDescription: string
  eventCenterWaiting: string
  eventCenterConnected: string
  eventCenterConnecting: string
  eventCenterDisconnected: string
  loading: string
  error: string
  status: Record<string, string>
  notificationStatus: Record<string, string>
  notificationReasonLabel: Record<string, string>
  eventType: Record<string, string>
  tool: Record<string, string>
}

export const supportedLanguages: LanguageCode[] = ['zh-CN', 'zh-TW', 'en', 'ja', 'ko', 'de']
export const languageStorageKey = 'niuma.language'

export const translations: Record<LanguageCode, Translation> = {
  'zh-CN': {
    languageName: '简体中文',
    appSubtitle: 'AI 编程工具状态控制台',
    mainStatus: '主状态',
    statusLegend: '状态图例',
    needsHandling: '需要处理',
    settings: '设置',
    statusTab: '状态',
    notificationTab: '通知',
    notificationPlugins: '通知插件',
    notificationSettings: '通知设置',
    barkSettings: 'Bark 设置',
    ntfySettings: 'ntfy 设置',
    enabled: '启用',
    disabled: '禁用',
    server: '服务地址',
    deviceKey: 'Key',
    group: '分组',
    topic: 'Topic',
    token: 'Token',
    save: '保存设置',
    saved: '已保存',
    saving: '保存中...',
    testSend: '测试通知',
    sending: '发送中...',
    sent: '已发送',
    lastResult: '最近结果',
    notificationRules: '通知规则',
    notificationHistory: '通知历史',
    notificationHealth: '通知健康',
    notificationChannelsReady: '可用渠道',
    latestNotification: '最近通知',
    noChannelsEnabled: '暂无启用渠道',
    listenerStatus: '监听状态',
    localSseInterface: '本地 SSE 接口',
    localSseState: '状态',
    localSsePort: '端口',
    localSsePath: '路径',
    localSseUrl: '访问地址',
    localSseConnected: '已连接',
    localSsePolling: '轮询中',
    expandNotificationSettings: '展开设置',
    collapseNotificationSettings: '收起设置',
    noNotificationRecords: '暂无通知记录',
    notificationChannel: '渠道',
    notificationTitle: '标题',
    notificationContent: '内容',
    notificationReason: '原因',
    notificationCreatedAt: '创建时间',
    notificationSentAt: '发送时间',
    notificationError: '错误',
    notifyApproval: '授权请求默认通知',
    notifyInput: '等待输入默认通知',
    notifyFailure: '任务失败默认通知',
    notifyCompletion: '任务完成时通知，用户主动中断和回滚除外',
    codexListening: 'Codex 监听',
    codexListenerDescription: '监听 Codex 阻塞通知并接收请求',
    toolListenerDescription: '启用工具插件后，NiumaNotifier 会监听对应工具的运行状态和阻塞请求。',
    codexListeningOn: '监听中',
    codexListeningOff: '未监听',
    listenerSaving: '保存中...',
    settingsButton: '设置',
    backToDashboard: '返回主界面',
    pluginManagement: '插件管理',
    pluginManagementDescription: '管理已发现的工具、通知和状态指示插件，启用后插件会按自身能力运行。',
    pluginConfig: '插件配置',
    importPlugin: '导入插件',
    importingPlugin: '正在导入...',
    pluginImportCancelled: '已取消导入',
    pluginImportSuccess: '插件已导入',
    removePlugin: '移除插件',
    pluginRemoveSuccess: '插件已移除',
    noPlugins: '暂无插件',
    pluginSource: '来源',
    pluginVersion: '版本',
    pluginRuntimeStatus: '运行状态',
    pluginInstallPath: '安装路径',
    pluginLastError: '最近错误',
    pluginCapabilities: '插件能力',
    pluginCapabilityEventWatcher: '事件监听',
    pluginCapabilityEventConsumer: '事件消费',
    pluginCapabilityApprovalHandler: '授权处理',
    pluginCapabilityNotificationTest: '通知测试',
    pluginCapabilityStateConsumer: '状态消费',
    pluginCapabilityToolSessionListProvider: '提供 AI 会话列表',
    pluginCapabilityToolSessionDetailProvider: '提供 AI 会话解析',
    pluginCapabilityToolSessionListReader: '读取 AI 会话列表',
    pluginCapabilityToolSessionDetailReader: '可读取 AI 会话内容',
    pluginStarting: '启动中',
    pluginRunning: '运行中',
    pluginStopped: '已停止',
    pluginStopping: '停止中',
    pluginFailed: '失败',
    pluginBuiltin: '内置',
    pluginExternal: '外部',
    language: '语言',
    refresh: '刷新',
    clearBlocker: '我已处理',
    clearBlockerAfterTool: '我已在 {tool} 中处理',
    clearBlockerConfirm:
      '这只会清除 NiumaNotifier 中当前所有待处理提醒，不会在 AI 工具中批准、拒绝或输入内容。',
    clearBlockerConfirmAgain: '再次点击确认',
    clearBlockerClearing: '处理中...',
    approveApproval: '同意',
    denyApproval: '拒绝',
    approvalSubmitting: '提交中...',
    submitInputAnswer: '提交输入',
    customInputAnswer: '自定义答案',
    inputTextPlaceholder: '请输入回复',
    sessionControlPlaceholder: '输入要发送给当前会话的指令',
    sessionControlSend: '发送',
    sessionControlInterrupt: '中断',
    sessionControlUnsupported: '当前会话不支持发送指令',
    sessionControlFailed: '控制请求失败',
    currentRequest: '当前请求',
    handlingHint: '处理提示',
    project: '项目',
    path: '路径',
    toolLabel: '工具',
    requestContent: '请求内容',
    requestTime: '请求时间',
    currentStatus: '当前状态',
    activeSession: '活跃 Session',
    sessionList: 'Session 列表',
    sessionOverview: 'Session 概览',
    projectName: '项目名称',
    sessionId: 'Session ID',
    lastActivity: '最后活动',
    latestEvent: '最近事件',
    recentEvents: '最近事件',
    noSessions: '暂无 Session',
    noSessionSelected: '未选择 Session',
    none: '暂无',
    noEvents: '暂无事件',
    eventCenter: '事件中心',
    eventCenterDescription: '只显示打开面板后收到的实时 NiumaEvent。',
    eventCenterWaiting: '等待新的实时事件',
    eventCenterConnected: '实时已连接',
    eventCenterConnecting: '实时连接中',
    eventCenterDisconnected: '实时已断开',
    loading: '加载中',
    error: '错误',
    status: {
      idle: '空闲',
      running: '正在运行',
      waiting_approval: '等待批准',
      waiting_input: '等待输入',
      completed: '完毕',
      error: '出错'
    },
    notificationStatus: {
      pending: '待发送',
      sent: '已发送',
      failed: '发送失败',
      skipped: '已跳过'
    },
    notificationReasonLabel: {
      manual_test: '手动测试',
      approval_requested: '请求审批',
      input_requested: '等待输入',
      task_failed: '任务失败',
      completed: '任务完成',
      unknown: '未知'
    },
    eventType: {
      session_started: 'Session 已开始',
      session_idled: 'Session 已空闲',
      approval_requested: '请求审批',
      input_requested: '请求输入',
      task_failed: '任务出错',
      assistant_message_completed: '回复已完成',
      manual_dismissed: '已标记处理'
    },
    tool: {
      codex: 'Codex',
      claude_code: 'Claude Code'
    }
  },
  'zh-TW': {
    languageName: '繁體中文',
    appSubtitle: 'AI 程式開發工具狀態控制台',
    mainStatus: '主狀態',
    statusLegend: '狀態圖例',
    needsHandling: '需要處理',
    settings: '設定',
    statusTab: '狀態',
    notificationTab: '通知',
    notificationPlugins: '通知外掛',
    notificationSettings: '通知設定',
    barkSettings: 'Bark 設定',
    ntfySettings: 'ntfy 設定',
    enabled: '啟用',
    disabled: '停用',
    server: '服務位址',
    deviceKey: 'Key',
    group: '分組',
    topic: 'Topic',
    token: 'Token',
    save: '儲存設定',
    saved: '已儲存',
    saving: '儲存中...',
    testSend: '測試通知',
    sending: '傳送中...',
    sent: '已傳送',
    lastResult: '最近結果',
    notificationRules: '通知規則',
    notificationHistory: '通知歷史',
    notificationHealth: '通知健康',
    notificationChannelsReady: '可用渠道',
    latestNotification: '最近通知',
    noChannelsEnabled: '暫無啟用渠道',
    listenerStatus: '監聽狀態',
    localSseInterface: '本地 SSE 介面',
    localSseState: '狀態',
    localSsePort: '連接埠',
    localSsePath: '路徑',
    localSseUrl: '存取位址',
    localSseConnected: '已連線',
    localSsePolling: '輪詢中',
    expandNotificationSettings: '展開設定',
    collapseNotificationSettings: '收合設定',
    noNotificationRecords: '暫無通知記錄',
    notificationChannel: '渠道',
    notificationTitle: '標題',
    notificationContent: '內容',
    notificationReason: '原因',
    notificationCreatedAt: '建立時間',
    notificationSentAt: '傳送時間',
    notificationError: '錯誤',
    notifyApproval: '授權請求預設通知',
    notifyInput: '等待輸入預設通知',
    notifyFailure: '任務失敗預設通知',
    notifyCompletion: '任務完成時通知，使用者主動中斷和回滾除外',
    codexListening: 'Codex 監聽',
    codexListenerDescription: '監聽 Codex 阻塞通知並接收請求',
    toolListenerDescription: '啟用工具外掛後，NiumaNotifier 會監聽對應工具的執行狀態和阻塞請求。',
    codexListeningOn: '監聽中',
    codexListeningOff: '未監聽',
    listenerSaving: '儲存中...',
    settingsButton: '設定',
    backToDashboard: '返回主畫面',
    pluginManagement: '外掛管理',
    pluginManagementDescription: '管理已發現的工具、通知和狀態指示外掛，啟用後外掛會依自身能力執行。',
    pluginConfig: '外掛設定',
    importPlugin: '匯入外掛',
    importingPlugin: '正在匯入...',
    pluginImportCancelled: '已取消匯入',
    pluginImportSuccess: '外掛已匯入',
    removePlugin: '移除外掛',
    pluginRemoveSuccess: '外掛已移除',
    noPlugins: '暫無外掛',
    pluginSource: '來源',
    pluginVersion: '版本',
    pluginRuntimeStatus: '執行狀態',
    pluginInstallPath: '安裝路徑',
    pluginLastError: '最近錯誤',
    pluginCapabilities: '外掛能力',
    pluginCapabilityEventWatcher: '事件監聽',
    pluginCapabilityEventConsumer: '事件消費',
    pluginCapabilityApprovalHandler: '授權處理',
    pluginCapabilityNotificationTest: '通知測試',
    pluginCapabilityStateConsumer: '狀態消費',
    pluginCapabilityToolSessionListProvider: '提供 AI 會話列表',
    pluginCapabilityToolSessionDetailProvider: '提供 AI 會話解析',
    pluginCapabilityToolSessionListReader: '讀取 AI 會話列表',
    pluginCapabilityToolSessionDetailReader: '可讀取 AI 會話內容',
    pluginStarting: '啟動中',
    pluginRunning: '執行中',
    pluginStopped: '已停止',
    pluginStopping: '停止中',
    pluginFailed: '失敗',
    pluginBuiltin: '內建',
    pluginExternal: '外部',
    language: '語言',
    refresh: '重新整理',
    clearBlocker: '我已處理',
    clearBlockerAfterTool: '我已在 {tool} 中處理',
    clearBlockerConfirm:
      '這只會清除 NiumaNotifier 中目前所有待處理提醒，不會在 AI 工具中批准、拒絕或輸入內容。',
    clearBlockerConfirmAgain: '再次點擊確認',
    clearBlockerClearing: '處理中...',
    approveApproval: '同意',
    denyApproval: '拒絕',
    approvalSubmitting: '提交中...',
    submitInputAnswer: '提交輸入',
    customInputAnswer: '自訂答案',
    inputTextPlaceholder: '請輸入回覆',
    sessionControlPlaceholder: '輸入要傳送給目前會話的指令',
    sessionControlSend: '傳送',
    sessionControlInterrupt: '中斷',
    sessionControlUnsupported: '目前會話不支援傳送指令',
    sessionControlFailed: '控制請求失敗',
    currentRequest: '目前請求',
    handlingHint: '處理提示',
    project: '專案',
    path: '路徑',
    toolLabel: '工具',
    requestContent: '請求內容',
    requestTime: '請求時間',
    currentStatus: '目前狀態',
    activeSession: '活躍 Session',
    sessionList: 'Session 列表',
    sessionOverview: 'Session 概覽',
    projectName: '專案名稱',
    sessionId: 'Session ID',
    lastActivity: '最後活動',
    latestEvent: '最近事件',
    recentEvents: '最近事件',
    noSessions: '暫無 Session',
    noSessionSelected: '未選擇 Session',
    none: '暫無',
    noEvents: '暫無事件',
    eventCenter: '事件中心',
    eventCenterDescription: '只顯示開啟面板後收到的即時 NiumaEvent。',
    eventCenterWaiting: '等待新的即時事件',
    eventCenterConnected: '即時已連線',
    eventCenterConnecting: '即時連線中',
    eventCenterDisconnected: '即時已斷線',
    loading: '載入中',
    error: '錯誤',
    status: {
      idle: '閒置',
      running: '執行中',
      waiting_approval: '等待審批',
      waiting_input: '等待輸入',
      completed: '已完成',
      error: '發生錯誤'
    },
    notificationStatus: {
      pending: '待傳送',
      sent: '已傳送',
      failed: '傳送失敗',
      skipped: '已跳過'
    },
    notificationReasonLabel: {
      manual_test: '手動測試',
      approval_requested: '請求審批',
      input_requested: '等待輸入',
      task_failed: '任務失敗',
      completed: '任務完成',
      unknown: '未知'
    },
    eventType: {
      session_started: 'Session 已開始',
      session_idled: 'Session 已閒置',
      approval_requested: '請求審批',
      input_requested: '請求輸入',
      task_failed: '任務出錯',
      assistant_message_completed: '回覆已完成',
      manual_dismissed: '已標記處理'
    },
    tool: {
      codex: 'Codex',
      claude_code: 'Claude Code'
    }
  },
  en: {
    languageName: 'English',
    appSubtitle: 'AI coding tool status console',
    mainStatus: 'Main status',
    statusLegend: 'Status legend',
    needsHandling: 'Needs handling',
    settings: 'Settings',
    statusTab: 'Status',
    notificationTab: 'Notifications',
    notificationPlugins: 'Notification plugins',
    notificationSettings: 'Notification settings',
    barkSettings: 'Bark settings',
    ntfySettings: 'ntfy settings',
    enabled: 'Enabled',
    disabled: 'Disabled',
    server: 'Server URL',
    deviceKey: 'Key',
    group: 'Group',
    topic: 'Topic',
    token: 'Token',
    save: 'Save settings',
    saved: 'Saved',
    saving: 'Saving...',
    testSend: 'Test notification',
    sending: 'Sending...',
    sent: 'Sent',
    lastResult: 'Last result',
    notificationRules: 'Notification rules',
    notificationHistory: 'Notification history',
    notificationHealth: 'Notification health',
    notificationChannelsReady: 'Ready channels',
    latestNotification: 'Latest notification',
    noChannelsEnabled: 'No enabled channels',
    listenerStatus: 'Listener status',
    localSseInterface: 'Local SSE interface',
    localSseState: 'Status',
    localSsePort: 'Port',
    localSsePath: 'Path',
    localSseUrl: 'Access URL',
    localSseConnected: 'Connected',
    localSsePolling: 'Polling',
    expandNotificationSettings: 'Expand settings',
    collapseNotificationSettings: 'Collapse settings',
    noNotificationRecords: 'No notification records',
    notificationChannel: 'Channel',
    notificationTitle: 'Title',
    notificationContent: 'Content',
    notificationReason: 'Reason',
    notificationCreatedAt: 'Created',
    notificationSentAt: 'Sent at',
    notificationError: 'Error',
    notifyApproval: 'Approval requests notify by default',
    notifyInput: 'Input requests notify by default',
    notifyFailure: 'Task failures notify by default',
    notifyCompletion: 'Task completions notify except user interruptions and rollbacks',
    codexListening: 'Codex listener',
    codexListenerDescription: 'Listen for Codex blocker notifications and requests',
    toolListenerDescription: 'Enable tool plugins to let NiumaNotifier listen for matching tool activity and blocker requests.',
    codexListeningOn: 'Listening',
    codexListeningOff: 'Not listening',
    listenerSaving: 'Saving...',
    settingsButton: 'Settings',
    backToDashboard: 'Back to dashboard',
    pluginManagement: 'Plugin management',
    pluginManagementDescription:
      'Manage discovered tool, notification, and status indicator plugins. Enabled plugins run according to their capabilities.',
    pluginConfig: 'Plugin config',
    importPlugin: 'Import plugin',
    importingPlugin: 'Importing...',
    pluginImportCancelled: 'Import cancelled',
    pluginImportSuccess: 'Plugin imported',
    removePlugin: 'Remove plugin',
    pluginRemoveSuccess: 'Plugin removed',
    noPlugins: 'No plugins',
    pluginSource: 'Source',
    pluginVersion: 'Version',
    pluginRuntimeStatus: 'Runtime status',
    pluginInstallPath: 'Install path',
    pluginLastError: 'Last error',
    pluginCapabilities: 'Capabilities',
    pluginCapabilityEventWatcher: 'Event watcher',
    pluginCapabilityEventConsumer: 'Event consumer',
    pluginCapabilityApprovalHandler: 'Approval handling',
    pluginCapabilityNotificationTest: 'Notification test',
    pluginCapabilityStateConsumer: 'State consumer',
    pluginCapabilityToolSessionListProvider: 'Provides AI session list',
    pluginCapabilityToolSessionDetailProvider: 'Provides AI session parsing',
    pluginCapabilityToolSessionListReader: 'Reads AI session list',
    pluginCapabilityToolSessionDetailReader: 'Can read AI session content',
    pluginStarting: 'Starting',
    pluginRunning: 'Running',
    pluginStopped: 'Stopped',
    pluginStopping: 'Stopping',
    pluginFailed: 'Failed',
    pluginBuiltin: 'Built-in',
    pluginExternal: 'External',
    language: 'Language',
    refresh: 'Refresh',
    clearBlocker: 'Handled',
    clearBlockerAfterTool: 'Handled in {tool}',
    clearBlockerConfirm:
      'This only clears all current attention reminders in NiumaNotifier. It does not approve, deny, or enter anything in the AI tool.',
    clearBlockerConfirmAgain: 'Click again to confirm',
    clearBlockerClearing: 'Marking handled...',
    approveApproval: 'Allow',
    denyApproval: 'Deny',
    approvalSubmitting: 'Submitting...',
    submitInputAnswer: 'Submit input',
    customInputAnswer: 'Custom answer',
    inputTextPlaceholder: 'Enter a response',
    sessionControlPlaceholder: 'Enter an instruction for this session',
    sessionControlSend: 'Send',
    sessionControlInterrupt: 'Interrupt',
    sessionControlUnsupported: 'This session does not support sending instructions',
    sessionControlFailed: 'Control request failed',
    currentRequest: 'Current request',
    handlingHint: 'Handling hint',
    project: 'Project',
    path: 'Path',
    toolLabel: 'Tool',
    requestContent: 'Request',
    requestTime: 'Request time',
    currentStatus: 'Current status',
    activeSession: 'Active session',
    sessionList: 'Sessions',
    sessionOverview: 'Session overview',
    projectName: 'Project name',
    sessionId: 'Session ID',
    lastActivity: 'Last activity',
    latestEvent: 'Latest event',
    recentEvents: 'Recent events',
    noSessions: 'No sessions',
    noSessionSelected: 'No session selected',
    none: 'None',
    noEvents: 'No events',
    eventCenter: 'Event center',
    eventCenterDescription: 'Shows only realtime NiumaEvent messages received after this panel opens.',
    eventCenterWaiting: 'Waiting for realtime events',
    eventCenterConnected: 'Realtime connected',
    eventCenterConnecting: 'Realtime connecting',
    eventCenterDisconnected: 'Realtime disconnected',
    loading: 'Loading',
    error: 'Error',
    status: {
      idle: 'Idle',
      running: 'Running',
      waiting_approval: 'Waiting for approval',
      waiting_input: 'Waiting for input',
      completed: 'Completed',
      error: 'Error'
    },
    notificationStatus: {
      pending: 'Pending',
      sent: 'Sent',
      failed: 'Failed',
      skipped: 'Skipped'
    },
    notificationReasonLabel: {
      manual_test: 'Manual test',
      approval_requested: 'Approval requested',
      input_requested: 'Input requested',
      task_failed: 'Task failed',
      completed: 'Task completed',
      unknown: 'Unknown'
    },
    eventType: {
      session_started: 'Session started',
      session_idled: 'Session idled',
      approval_requested: 'Approval requested',
      input_requested: 'Input requested',
      task_failed: 'Task failed',
      assistant_message_completed: 'Reply completed',
      manual_dismissed: 'Marked handled'
    },
    tool: {
      codex: 'Codex',
      claude_code: 'Claude Code'
    }
  },
  ja: {
    languageName: '日本語',
    appSubtitle: 'AI コーディングツール状態コンソール',
    mainStatus: 'メイン状態',
    statusLegend: '状態凡例',
    needsHandling: '対応が必要',
    settings: '設定',
    statusTab: '状態',
    notificationTab: '通知',
    notificationPlugins: '通知プラグイン',
    notificationSettings: '通知設定',
    barkSettings: 'Bark 設定',
    ntfySettings: 'ntfy 設定',
    enabled: '有効',
    disabled: '無効',
    server: 'サーバー URL',
    deviceKey: 'Key',
    group: 'グループ',
    topic: 'Topic',
    token: 'Token',
    save: '設定を保存',
    saved: '保存済み',
    saving: '保存中...',
    testSend: '通知をテスト',
    sending: '送信中...',
    sent: '送信済み',
    lastResult: '直近の結果',
    notificationRules: '通知ルール',
    notificationHistory: '通知履歴',
    notificationHealth: '通知ヘルス',
    notificationChannelsReady: '利用可能チャンネル',
    latestNotification: '最新通知',
    noChannelsEnabled: '有効なチャンネルなし',
    listenerStatus: '監視状態',
    localSseInterface: 'ローカル SSE インターフェイス',
    localSseState: '状態',
    localSsePort: 'ポート',
    localSsePath: 'パス',
    localSseUrl: 'アクセス URL',
    localSseConnected: '接続済み',
    localSsePolling: 'ポーリング中',
    expandNotificationSettings: '設定を展開',
    collapseNotificationSettings: '設定を折りたたむ',
    noNotificationRecords: '通知記録はありません',
    notificationChannel: 'チャンネル',
    notificationTitle: 'タイトル',
    notificationContent: '内容',
    notificationReason: '理由',
    notificationCreatedAt: '作成時刻',
    notificationSentAt: '送信時刻',
    notificationError: 'エラー',
    notifyApproval: '承認リクエストは既定で通知',
    notifyInput: '入力待ちは既定で通知',
    notifyFailure: 'タスク失敗は既定で通知',
    notifyCompletion: 'タスク完了時に通知、ユーザー中断とロールバックは除外',
    codexListening: 'Codex 監視',
    codexListenerDescription: 'Codex のブロック通知とリクエストを監視',
    toolListenerDescription: 'ツールプラグインを有効にすると、対応するツールの実行状態とブロック要求を監視します。',
    codexListeningOn: '監視中',
    codexListeningOff: '未監視',
    listenerSaving: '保存中...',
    settingsButton: '設定',
    backToDashboard: 'メイン画面へ戻る',
    pluginManagement: 'プラグイン管理',
    pluginManagementDescription:
      '検出されたツール、通知、ステータス表示プラグインを管理します。有効なプラグインはそれぞれの機能に応じて動作します。',
    pluginConfig: 'プラグイン設定',
    importPlugin: 'プラグインを取り込む',
    importingPlugin: '取り込み中...',
    pluginImportCancelled: '取り込みをキャンセルしました',
    pluginImportSuccess: 'プラグインを取り込みました',
    removePlugin: 'プラグインを削除',
    pluginRemoveSuccess: 'プラグインを削除しました',
    noPlugins: 'プラグインはありません',
    pluginSource: '提供元',
    pluginVersion: 'バージョン',
    pluginRuntimeStatus: '実行状態',
    pluginInstallPath: 'インストール先',
    pluginLastError: '最近のエラー',
    pluginCapabilities: '機能',
    pluginCapabilityEventWatcher: 'イベント監視',
    pluginCapabilityEventConsumer: 'イベント消費',
    pluginCapabilityApprovalHandler: '承認処理',
    pluginCapabilityNotificationTest: '通知テスト',
    pluginCapabilityStateConsumer: '状態消費',
    pluginCapabilityToolSessionListProvider: 'AI セッション一覧を提供',
    pluginCapabilityToolSessionDetailProvider: 'AI セッション解析を提供',
    pluginCapabilityToolSessionListReader: 'AI セッション一覧を読み取り',
    pluginCapabilityToolSessionDetailReader: 'AI セッション内容を読み取り可能',
    pluginStarting: '起動中',
    pluginRunning: '実行中',
    pluginStopped: '停止中',
    pluginStopping: '停止処理中',
    pluginFailed: '失敗',
    pluginBuiltin: '内蔵',
    pluginExternal: '外部',
    language: '言語',
    refresh: '更新',
    clearBlocker: '処理済み',
    clearBlockerAfterTool: '{tool} で処理済み',
    clearBlockerConfirm:
      'これは NiumaNotifier の現在の未処理リマインダーをすべて消すだけです。AI ツールで承認、拒否、入力は行いません。',
    clearBlockerConfirmAgain: 'もう一度クリックして確認',
    clearBlockerClearing: '処理済みにしています...',
    approveApproval: '承認',
    denyApproval: '拒否',
    approvalSubmitting: '送信中...',
    submitInputAnswer: '入力を送信',
    customInputAnswer: 'カスタム回答',
    inputTextPlaceholder: '返信を入力',
    sessionControlPlaceholder: 'このセッションに送信する指示を入力',
    sessionControlSend: '送信',
    sessionControlInterrupt: '中断',
    sessionControlUnsupported: 'このセッションは指示の送信に対応していません',
    sessionControlFailed: '制御リクエストに失敗しました',
    currentRequest: '現在のリクエスト',
    handlingHint: '処理方法',
    project: 'プロジェクト',
    path: 'パス',
    toolLabel: 'ツール',
    requestContent: 'リクエスト内容',
    requestTime: 'リクエスト時刻',
    currentStatus: '現在の状態',
    activeSession: 'アクティブ Session',
    sessionList: 'Session 一覧',
    sessionOverview: 'Session 概要',
    projectName: 'プロジェクト名',
    sessionId: 'Session ID',
    lastActivity: '最終アクティビティ',
    latestEvent: '最新イベント',
    recentEvents: '最近のイベント',
    noSessions: 'Session なし',
    noSessionSelected: 'Session が選択されていません',
    none: 'なし',
    noEvents: 'イベントなし',
    eventCenter: 'イベントセンター',
    eventCenterDescription: 'このパネルを開いた後に受信したリアルタイム NiumaEvent のみを表示します。',
    eventCenterWaiting: '新しいリアルタイムイベントを待機中',
    eventCenterConnected: 'リアルタイム接続済み',
    eventCenterConnecting: 'リアルタイム接続中',
    eventCenterDisconnected: 'リアルタイム切断',
    loading: '読み込み中',
    error: 'エラー',
    status: {
      idle: 'アイドル',
      running: '実行中',
      waiting_approval: '承認待ち',
      waiting_input: '入力待ち',
      completed: '完了',
      error: 'エラー'
    },
    notificationStatus: {
      pending: '送信待ち',
      sent: '送信済み',
      failed: '送信失敗',
      skipped: 'スキップ済み'
    },
    notificationReasonLabel: {
      manual_test: '手動テスト',
      approval_requested: '承認リクエスト',
      input_requested: '入力待ち',
      task_failed: 'タスク失敗',
      completed: 'タスク完了',
      unknown: '不明'
    },
    eventType: {
      session_started: 'Session 開始',
      session_idled: 'Session アイドル',
      approval_requested: '承認リクエスト',
      input_requested: '入力リクエスト',
      task_failed: 'タスクエラー',
      assistant_message_completed: '返信完了',
      manual_dismissed: '処理済みに設定'
    },
    tool: {
      codex: 'Codex',
      claude_code: 'Claude Code'
    }
  },
  ko: {
    languageName: '한국어',
    appSubtitle: 'AI 코딩 도구 상태 콘솔',
    mainStatus: '주 상태',
    statusLegend: '상태 범례',
    needsHandling: '처리 필요',
    settings: '설정',
    statusTab: '상태',
    notificationTab: '알림',
    notificationPlugins: '알림 플러그인',
    notificationSettings: '알림 설정',
    barkSettings: 'Bark 설정',
    ntfySettings: 'ntfy 설정',
    enabled: '사용',
    disabled: '사용 안 함',
    server: '서버 URL',
    deviceKey: 'Key',
    group: '그룹',
    topic: 'Topic',
    token: 'Token',
    save: '설정 저장',
    saved: '저장됨',
    saving: '저장 중...',
    testSend: '알림 테스트',
    sending: '전송 중...',
    sent: '전송됨',
    lastResult: '최근 결과',
    notificationRules: '알림 규칙',
    notificationHistory: '알림 기록',
    notificationHealth: '알림 상태',
    notificationChannelsReady: '사용 가능 채널',
    latestNotification: '최근 알림',
    noChannelsEnabled: '사용 중인 채널 없음',
    listenerStatus: '수신 상태',
    localSseInterface: '로컬 SSE 인터페이스',
    localSseState: '상태',
    localSsePort: '포트',
    localSsePath: '경로',
    localSseUrl: '접속 주소',
    localSseConnected: '연결됨',
    localSsePolling: '폴링 중',
    expandNotificationSettings: '설정 펼치기',
    collapseNotificationSettings: '설정 접기',
    noNotificationRecords: '알림 기록 없음',
    notificationChannel: '채널',
    notificationTitle: '제목',
    notificationContent: '내용',
    notificationReason: '사유',
    notificationCreatedAt: '생성 시간',
    notificationSentAt: '전송 시간',
    notificationError: '오류',
    notifyApproval: '승인 요청은 기본 알림',
    notifyInput: '입력 대기는 기본 알림',
    notifyFailure: '작업 실패는 기본 알림',
    notifyCompletion: '작업 완료 시 알림, 사용자 중단 및 롤백 제외',
    codexListening: 'Codex 수신',
    codexListenerDescription: 'Codex 차단 알림과 요청을 수신',
    toolListenerDescription: '도구 플러그인을 활성화하면 NiumaNotifier가 해당 도구의 실행 상태와 차단 요청을 수신합니다.',
    codexListeningOn: '수신 중',
    codexListeningOff: '수신 안 함',
    listenerSaving: '저장 중...',
    settingsButton: '설정',
    backToDashboard: '메인 화면으로 돌아가기',
    pluginManagement: '플러그인 관리',
    pluginManagementDescription:
      '발견된 도구, 알림 및 상태 표시 플러그인을 관리합니다. 활성화된 플러그인은 각 기능에 따라 실행됩니다.',
    pluginConfig: '플러그인 설정',
    importPlugin: '플러그인 가져오기',
    importingPlugin: '가져오는 중...',
    pluginImportCancelled: '가져오기를 취소했습니다',
    pluginImportSuccess: '플러그인을 가져왔습니다',
    removePlugin: '플러그인 제거',
    pluginRemoveSuccess: '플러그인을 제거했습니다',
    noPlugins: '플러그인이 없습니다',
    pluginSource: '출처',
    pluginVersion: '버전',
    pluginRuntimeStatus: '실행 상태',
    pluginInstallPath: '설치 경로',
    pluginLastError: '최근 오류',
    pluginCapabilities: '플러그인 기능',
    pluginCapabilityEventWatcher: '이벤트 수신',
    pluginCapabilityEventConsumer: '이벤트 소비',
    pluginCapabilityApprovalHandler: '승인 처리',
    pluginCapabilityNotificationTest: '알림 테스트',
    pluginCapabilityStateConsumer: '상태 소비',
    pluginCapabilityToolSessionListProvider: 'AI 세션 목록 제공',
    pluginCapabilityToolSessionDetailProvider: 'AI 세션 해석 제공',
    pluginCapabilityToolSessionListReader: 'AI 세션 목록 읽기',
    pluginCapabilityToolSessionDetailReader: 'AI 세션 내용을 읽을 수 있음',
    pluginStarting: '시작 중',
    pluginRunning: '실행 중',
    pluginStopped: '중지됨',
    pluginStopping: '중지 중',
    pluginFailed: '실패',
    pluginBuiltin: '내장',
    pluginExternal: '외부',
    language: '언어',
    refresh: '새로고침',
    clearBlocker: '처리 완료',
    clearBlockerAfterTool: '{tool}에서 처리 완료',
    clearBlockerConfirm:
      '이 작업은 NiumaNotifier의 현재 처리 필요 알림만 모두 지웁니다. AI 도구에서 승인, 거부 또는 입력을 수행하지 않습니다.',
    clearBlockerConfirmAgain: '다시 클릭해 확인',
    clearBlockerClearing: '처리 중...',
    approveApproval: '승인',
    denyApproval: '거부',
    approvalSubmitting: '제출 중...',
    submitInputAnswer: '입력 제출',
    customInputAnswer: '직접 입력',
    inputTextPlaceholder: '응답을 입력하세요',
    sessionControlPlaceholder: '이 세션에 보낼 지시를 입력하세요',
    sessionControlSend: '보내기',
    sessionControlInterrupt: '중단',
    sessionControlUnsupported: '이 세션은 지시 보내기를 지원하지 않습니다',
    sessionControlFailed: '제어 요청 실패',
    currentRequest: '현재 요청',
    handlingHint: '처리 안내',
    project: '프로젝트',
    path: '경로',
    toolLabel: '도구',
    requestContent: '요청 내용',
    requestTime: '요청 시간',
    currentStatus: '현재 상태',
    activeSession: '활성 Session',
    sessionList: 'Session 목록',
    sessionOverview: 'Session 개요',
    projectName: '프로젝트 이름',
    sessionId: 'Session ID',
    lastActivity: '마지막 활동',
    latestEvent: '최근 이벤트',
    recentEvents: '최근 이벤트',
    noSessions: 'Session 없음',
    noSessionSelected: '선택된 Session 없음',
    none: '없음',
    noEvents: '이벤트 없음',
    eventCenter: '이벤트 센터',
    eventCenterDescription: '이 패널을 연 뒤 받은 실시간 NiumaEvent만 표시합니다.',
    eventCenterWaiting: '새 실시간 이벤트 대기 중',
    eventCenterConnected: '실시간 연결됨',
    eventCenterConnecting: '실시간 연결 중',
    eventCenterDisconnected: '실시간 연결 끊김',
    loading: '로딩 중',
    error: '오류',
    status: {
      idle: '유휴',
      running: '실행 중',
      waiting_approval: '승인 대기',
      waiting_input: '입력 대기',
      completed: '완료됨',
      error: '오류'
    },
    notificationStatus: {
      pending: '전송 대기',
      sent: '전송됨',
      failed: '전송 실패',
      skipped: '건너뜀'
    },
    notificationReasonLabel: {
      manual_test: '수동 테스트',
      approval_requested: '승인 요청',
      input_requested: '입력 대기',
      task_failed: '작업 실패',
      completed: '작업 완료',
      unknown: '알 수 없음'
    },
    eventType: {
      session_started: 'Session 시작됨',
      session_idled: 'Session 유휴 상태',
      approval_requested: '승인 요청됨',
      input_requested: '입력 요청됨',
      task_failed: '작업 오류',
      assistant_message_completed: '응답 완료됨',
      manual_dismissed: '처리됨으로 표시'
    },
    tool: {
      codex: 'Codex',
      claude_code: 'Claude Code'
    }
  },
  de: {
    languageName: 'Deutsch',
    appSubtitle: 'Statuskonsole für KI-Coding-Tools',
    mainStatus: 'Hauptstatus',
    statusLegend: 'Statuslegende',
    needsHandling: 'Aktion erforderlich',
    settings: 'Einstellungen',
    statusTab: 'Status',
    notificationTab: 'Benachrichtigungen',
    notificationPlugins: 'Benachrichtigungs-Plugins',
    notificationSettings: 'Benachrichtigungseinstellungen',
    barkSettings: 'Bark-Einstellungen',
    ntfySettings: 'ntfy-Einstellungen',
    enabled: 'Aktiviert',
    disabled: 'Deaktiviert',
    server: 'Server-URL',
    deviceKey: 'Key',
    group: 'Gruppe',
    topic: 'Topic',
    token: 'Token',
    save: 'Einstellungen speichern',
    saved: 'Gespeichert',
    saving: 'Speichern...',
    testSend: 'Benachrichtigung testen',
    sending: 'Wird gesendet...',
    sent: 'Gesendet',
    lastResult: 'Letztes Ergebnis',
    notificationRules: 'Benachrichtigungsregeln',
    notificationHistory: 'Benachrichtigungsverlauf',
    notificationHealth: 'Benachrichtigungsstatus',
    notificationChannelsReady: 'Bereite Kanäle',
    latestNotification: 'Letzte Benachrichtigung',
    noChannelsEnabled: 'Keine aktiven Kanäle',
    listenerStatus: 'Überwachungsstatus',
    localSseInterface: 'Lokale SSE-Schnittstelle',
    localSseState: 'Status',
    localSsePort: 'Port',
    localSsePath: 'Pfad',
    localSseUrl: 'Zugriffsadresse',
    localSseConnected: 'Verbunden',
    localSsePolling: 'Polling',
    expandNotificationSettings: 'Einstellungen öffnen',
    collapseNotificationSettings: 'Einstellungen schließen',
    noNotificationRecords: 'Keine Benachrichtigungen',
    notificationChannel: 'Kanal',
    notificationTitle: 'Titel',
    notificationContent: 'Inhalt',
    notificationReason: 'Grund',
    notificationCreatedAt: 'Erstellt',
    notificationSentAt: 'Gesendet um',
    notificationError: 'Fehler',
    notifyApproval: 'Freigabeanfragen werden standardmäßig benachrichtigt',
    notifyInput: 'Eingabeanfragen werden standardmäßig benachrichtigt',
    notifyFailure: 'Fehlgeschlagene Aufgaben werden standardmäßig benachrichtigt',
    notifyCompletion: 'Aufgabenabschlüsse werden benachrichtigt, außer Benutzerabbruch und Rollback',
    codexListening: 'Codex-Überwachung',
    codexListenerDescription: 'Codex-Blocker-Benachrichtigungen und Anfragen überwachen',
    toolListenerDescription: 'Aktivierte Tool-Plugins lassen NiumaNotifier passende Tool-Aktivitäten und Blocker-Anfragen überwachen.',
    codexListeningOn: 'Aktiv',
    codexListeningOff: 'Inaktiv',
    listenerSaving: 'Speichert...',
    settingsButton: 'Einstellungen',
    backToDashboard: 'Zurück zur Übersicht',
    pluginManagement: 'Plugin-Verwaltung',
    pluginManagementDescription:
      'Erkannte Tool-, Benachrichtigungs- und Statusanzeige-Plugins verwalten. Aktivierte Plugins laufen entsprechend ihren Fähigkeiten.',
    pluginConfig: 'Plugin-Konfiguration',
    importPlugin: 'Plugin importieren',
    importingPlugin: 'Importiert...',
    pluginImportCancelled: 'Import abgebrochen',
    pluginImportSuccess: 'Plugin importiert',
    removePlugin: 'Plugin entfernen',
    pluginRemoveSuccess: 'Plugin entfernt',
    noPlugins: 'Keine Plugins',
    pluginSource: 'Quelle',
    pluginVersion: 'Version',
    pluginRuntimeStatus: 'Laufzeitstatus',
    pluginInstallPath: 'Installationspfad',
    pluginLastError: 'Letzter Fehler',
    pluginCapabilities: 'Fähigkeiten',
    pluginCapabilityEventWatcher: 'Ereignisüberwachung',
    pluginCapabilityEventConsumer: 'Ereigniskonsum',
    pluginCapabilityApprovalHandler: 'Genehmigungen',
    pluginCapabilityNotificationTest: 'Benachrichtigungstest',
    pluginCapabilityStateConsumer: 'Statuskonsum',
    pluginCapabilityToolSessionListProvider: 'Stellt KI-Session-Liste bereit',
    pluginCapabilityToolSessionDetailProvider: 'Stellt KI-Session-Analyse bereit',
    pluginCapabilityToolSessionListReader: 'Liest KI-Session-Liste',
    pluginCapabilityToolSessionDetailReader: 'Kann KI-Session-Inhalte lesen',
    pluginStarting: 'Startet',
    pluginRunning: 'Läuft',
    pluginStopped: 'Gestoppt',
    pluginStopping: 'Stoppt',
    pluginFailed: 'Fehlgeschlagen',
    pluginBuiltin: 'Integriert',
    pluginExternal: 'Extern',
    language: 'Sprache',
    refresh: 'Aktualisieren',
    clearBlocker: 'Erledigt',
    clearBlockerAfterTool: 'In {tool} erledigt',
    clearBlockerConfirm:
      'Dies entfernt nur alle aktuellen Hinweise in NiumaNotifier. Im KI-Tool wird nichts genehmigt, abgelehnt oder eingegeben.',
    clearBlockerConfirmAgain: 'Zum Bestätigen erneut klicken',
    clearBlockerClearing: 'Wird erledigt markiert...',
    approveApproval: 'Erlauben',
    denyApproval: 'Ablehnen',
    approvalSubmitting: 'Wird gesendet...',
    submitInputAnswer: 'Eingabe senden',
    customInputAnswer: 'Eigene Antwort',
    inputTextPlaceholder: 'Antwort eingeben',
    sessionControlPlaceholder: 'Anweisung für diese Sitzung eingeben',
    sessionControlSend: 'Senden',
    sessionControlInterrupt: 'Unterbrechen',
    sessionControlUnsupported: 'Diese Sitzung unterstützt das Senden von Anweisungen nicht',
    sessionControlFailed: 'Steuerungsanfrage fehlgeschlagen',
    currentRequest: 'Aktuelle Anfrage',
    handlingHint: 'Hinweis',
    project: 'Projekt',
    path: 'Pfad',
    toolLabel: 'Tool',
    requestContent: 'Anfrage',
    requestTime: 'Anfragezeit',
    currentStatus: 'Aktueller Status',
    activeSession: 'Aktive Session',
    sessionList: 'Sessions',
    sessionOverview: 'Session-Überblick',
    projectName: 'Projektname',
    sessionId: 'Session-ID',
    lastActivity: 'Letzte Aktivität',
    latestEvent: 'Letztes Ereignis',
    recentEvents: 'Letzte Ereignisse',
    noSessions: 'Keine Sessions',
    noSessionSelected: 'Keine Session ausgewählt',
    none: 'Keine',
    noEvents: 'Keine Ereignisse',
    eventCenter: 'Ereigniszentrum',
    eventCenterDescription: 'Zeigt nur Echtzeit-NiumaEvent-Meldungen, die nach dem Öffnen dieses Bereichs eingehen.',
    eventCenterWaiting: 'Warten auf Echtzeitereignisse',
    eventCenterConnected: 'Echtzeit verbunden',
    eventCenterConnecting: 'Echtzeit verbindet',
    eventCenterDisconnected: 'Echtzeit getrennt',
    loading: 'Lädt',
    error: 'Fehler',
    status: {
      idle: 'Leerlauf',
      running: 'Läuft',
      waiting_approval: 'Wartet auf Freigabe',
      waiting_input: 'Wartet auf Eingabe',
      completed: 'Abgeschlossen',
      error: 'Fehler'
    },
    notificationStatus: {
      pending: 'Ausstehend',
      sent: 'Gesendet',
      failed: 'Fehlgeschlagen',
      skipped: 'Übersprungen'
    },
    notificationReasonLabel: {
      manual_test: 'Manueller Test',
      approval_requested: 'Freigabe angefordert',
      input_requested: 'Eingabe angefordert',
      task_failed: 'Aufgabe fehlgeschlagen',
      completed: 'Aufgabe abgeschlossen',
      unknown: 'Unbekannt'
    },
    eventType: {
      session_started: 'Session gestartet',
      session_idled: 'Session im Leerlauf',
      approval_requested: 'Freigabe angefordert',
      input_requested: 'Eingabe angefordert',
      task_failed: 'Aufgabe fehlgeschlagen',
      assistant_message_completed: 'Antwort abgeschlossen',
      manual_dismissed: 'Als erledigt markiert'
    },
    tool: {
      codex: 'Codex',
      claude_code: 'Claude Code'
    }
  }
}

export function detectInitialLanguage(): LanguageCode {
  const storedLanguage = window.localStorage.getItem(languageStorageKey)
  if (storedLanguage) {
    return normalizeLanguage(storedLanguage)
  }
  for (const language of navigator.languages.length ? navigator.languages : [navigator.language]) {
    const normalized = normalizeLanguage(language)
    if (supportedLanguages.includes(normalized)) {
      return normalized
    }
  }
  return 'en'
}

export function normalizeLanguage(value: string): LanguageCode {
  const normalized = value.replace('_', '-').toLowerCase()
  if (normalized === 'zh-cn' || normalized === 'zh-hans' || normalized.startsWith('zh-hans-')) {
    return 'zh-CN'
  }
  if (
    normalized === 'zh-tw' ||
    normalized === 'zh-hk' ||
    normalized === 'zh-mo' ||
    normalized === 'zh-hant' ||
    normalized.startsWith('zh-hant-')
  ) {
    return 'zh-TW'
  }
  if (normalized.startsWith('ja')) {
    return 'ja'
  }
  if (normalized.startsWith('ko')) {
    return 'ko'
  }
  if (normalized.startsWith('de')) {
    return 'de'
  }
  if (normalized.startsWith('en')) {
    return 'en'
  }
  return 'en'
}

export function translateStatus(language: LanguageCode, status: string) {
  return translations[language].status[status] ?? status
}

export function translateNotificationStatus(language: LanguageCode, status: string) {
  return translations[language].notificationStatus[status] ?? status
}

export function translateNotificationReason(language: LanguageCode, reason: string | null) {
  const key = reason || 'unknown'
  return translations[language].notificationReasonLabel[key] ?? key
}

export function translateEventType(language: LanguageCode, eventType: string) {
  return translations[language].eventType[eventType] ?? eventType
}

export function translateTool(language: LanguageCode, tool: string) {
  return translations[language].tool[tool] ?? tool
}

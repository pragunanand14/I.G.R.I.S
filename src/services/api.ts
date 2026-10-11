import type { AiStatus } from "@/types/ai";
import type { Attachment, AttachmentData } from "@/types/attachments";
import type {
  AppInfo,
  ImportResult,
  PublicConfig,
  ReloadResult,
} from "@/types/app";
import type { Conversation, ConversationDetail } from "@/types/chat";
import type { Memory, MemoryKind } from "@/types/memory";
import type {
  CalendarEvent,
  EventInput,
  ParsedTime,
  Reminder,
  Task,
  TaskInput,
} from "@/types/productivity";
import type { VoiceStatus } from "@/types/voice";
import type {
  AllowedFolder,
  DetectedProject,
  FolderSuggestion,
  Project,
  ProjectInput,
} from "@/types/workspace";
import type { Settings, SettingsPatch } from "@/types/settings";
import type { SystemSnapshot } from "@/types/system";
import type { Briefing, Place } from "@/types/briefing";
import type { GoogleStatus } from "@/types/google";
import type { Routine, RoutineRun } from "@/types/routines";
import type {
  Expense,
  PeopleView,
  Person,
  SpendingPeriod,
  SpendingView,
} from "@/types/life";
import type { MediaAction, NowPlaying } from "@/types/media";
import type {
  AllowedAction,
  AppCandidate,
  AppEntry,
  AuditEntry,
  PermissionRule,
  ToolActivity,
  ToolInfo,
} from "@/types/tools";
import { call } from "./backend";
import type {
  OperatorAction,
  OperatorSnapshot,
  OperatorTask,
} from "@/types/operator";
import type { TaskControl, TaskEventRow, TaskInfo } from "@/types/task";
import type {
  FoundDevice,
  HomeDevice,
  PointerEvent as TvPointerEvent,
  TvActionName,
  TvState,
} from "@/types/home";
import type {
  DeviceAuditRow,
  DeviceSettings,
  Handoff,
  DevicesOverview,
  PairedDevice,
  PairingCode,
  RemoteTask,
  ThisDevice,
} from "@/types/devices";

/** All IPC commands in one place — mirrors `generate_handler!` in `src-tauri/src/lib.rs`. */
export const api = {
  getAppInfo: () => call<AppInfo>("get_app_info"),
  getConfigStatus: () => call<PublicConfig>("get_config_status"),
  getSettings: () => call<Settings>("get_settings"),
  updateSettings: (patch: SettingsPatch) =>
    call<Settings>("update_settings", { patch }),
  getSystemSnapshot: () => call<SystemSnapshot>("get_system_snapshot"),
  getAiStatus: () => call<AiStatus>("get_ai_status"),
  reloadConfig: () => call<ReloadResult>("reload_config"),
  phoneMessagesAccess: () => call<boolean>("phone_messages_access"),
  phoneMessagesSettings: () => call<void>("phone_messages_settings"),
  phoneMessagesAllApps: () => call<boolean>("phone_messages_all_apps"),
  phoneSetMessagesAllApps: (on: boolean) =>
    call<boolean>("phone_set_messages_all_apps", { on }),
  phoneScreenAccess: () => call<boolean>("phone_screen_access"),
  phoneScreenSettings: () => call<void>("phone_screen_settings"),
  /** What a home-screen widget asked for when it opened IGRIS ("listen", "widgets"), once. */
  widgetLaunchAction: () => call<string | null>("widget_launch_action"),
  widgetWeatherPlace: () => call<string | null>("widget_weather_place"),
  widgetSetWeatherPlace: (query: string) =>
    call<{ name: string; latitude: number; longitude: number }>(
      "widget_set_weather_place",
      { query },
    ),
  briefingGet: () => call<Briefing>("briefing_get"),
  briefingSet: (briefing: Briefing) =>
    call<Briefing>("briefing_set", { briefing }),
  /** Today's briefing run, once it has run. */
  briefingToday: () => call<RoutineRun | null>("briefing_today"),
  weatherHome: () => call<Place | null>("weather_home"),
  weatherSetHome: (query: string) => call<Place>("weather_set_home", { query }),
  peopleList: () => call<PeopleView>("people_list"),
  personSave: (p: {
    name: string;
    relation: string;
    birthday: string;
    notes: string;
  }) => call<Person>("person_save", p),
  personForget: (name: string) => call<void>("person_forget", { name }),
  spendingView: (period: SpendingPeriod) =>
    call<SpendingView>("spending_view", { period }),
  expenseSetCategory: (id: number, category: string) =>
    call<Expense>("expense_set_category", { id, category }),
  expenseDelete: (id: number) => call<void>("expense_delete", { id }),
  /** Look for payments now (phone messages and the last 30 days of Gmail). */
  expensesScan: () => call<{ added: number; notes: string[] }>("expenses_scan"),
  expensesSetAuto: (on: boolean) => call<void>("expenses_set_auto", { on }),
  routinesList: () => call<Routine[]>("routines_list"),
  routineRuns: (id?: number) => call<RoutineRun[]>("routine_runs", { id }),
  routineSetEnabled: (id: number, enabled: boolean) =>
    call<unknown>("routine_set_enabled", { id, enabled }),
  routineDelete: (id: number) => call<void>("routine_delete", { id }),
  routineRunNow: (id: number) => call<void>("routine_run_now", { id }),
  googleStatus: () => call<GoogleStatus>("google_status"),
  /** Opens Google's sign-in in the browser; resolves when the user finishes (up to 5 minutes). */
  googleConnect: () => call<GoogleStatus>("google_connect"),
  googleDisconnect: () => call<GoogleStatus>("google_disconnect"),
  mediaNowPlaying: () => call<NowPlaying>("media_now_playing"),
  mediaControl: (action: MediaAction) =>
    call<NowPlaying>("media_control", { action }),
  /** Pick a settings (.env) file and make it IGRIS's; `null` when the picker was cancelled. */
  importSettingsFile: () => call<ImportResult | null>("import_settings_file"),
  listConversations: () => call<Conversation[]>("list_conversations"),
  getConversation: (id: string) =>
    call<ConversationDetail>("get_conversation", { id }),
  renameConversation: (id: string, title: string) =>
    call<Conversation>("rename_conversation", { id, title }),
  deleteConversation: (id: string) => call<void>("delete_conversation", { id }),
  cancelChat: (requestId: string) =>
    call<boolean>("chat_cancel", { requestId }),
  /** The bytes go as base64 in JSON: raw IPC bodies don't arrive on every platform (Android's IPC turns them into JSON). */
  attachFile: async (file: File) =>
    call<Attachment>("attach_file", {
      name: file.name,
      data: await fileToBase64(file),
    }),
  discardAttachment: (id: string) => call<void>("discard_attachment", { id }),
  readAttachment: (id: string) =>
    call<AttachmentData>("read_attachment", { id }),
  getOperatorState: () => call<OperatorSnapshot>("get_operator_state"),
  operatorControl: (action: OperatorAction) =>
    call<OperatorSnapshot>("operator_control", { action }),
  listOperatorTasks: (limit?: number) =>
    call<OperatorTask[]>("list_operator_tasks", { limit }),
  listConversationTasks: (conversationId: string, limit?: number) =>
    call<TaskInfo[]>("list_conversation_tasks", { conversationId, limit }),
  taskControl: (taskId: string, action: TaskControl) =>
    call<TaskInfo>("task_control", { taskId, action }),
  listTaskEvents: (taskId: string, limit?: number) =>
    call<TaskEventRow[]>("list_task_events", { taskId, limit }),
  trustConversation: (conversationId: string) =>
    call<void>("trust_conversation", { conversationId }),
  /** `always`: also remember this kind of action ("Always allow"). */
  respondToolApproval: (callId: string, approved: boolean, always = false) =>
    call<boolean>("respond_tool_approval", { callId, approved, always }),
  listAllowedActions: () => call<AllowedAction[]>("list_permission_rules"),
  removeAllowedAction: (id: string) =>
    call<void>("remove_permission_rule", { id }),
  clearAllowedActions: () => call<number>("clear_permission_rules"),
  listTools: () => call<ToolInfo[]>("list_tools"),
  listToolAudit: (limit?: number) =>
    call<AuditEntry[]>("list_tool_audit", { limit }),
  getPermissionPolicy: () => call<PermissionRule[]>("get_permission_policy"),
  listApplications: () => call<AppEntry[]>("list_applications"),
  addApplication: (name: string, path: string) =>
    call<AppEntry>("add_application", { name, path }),
  removeApplication: (id: string) => call<void>("remove_application", { id }),
  detectApplications: () => call<AppCandidate[]>("detect_applications"),
  launchApplication: (id: string) =>
    call<ToolActivity>("launch_application", { id }),
  listMemories: (kind?: MemoryKind, query?: string) =>
    call<Memory[]>("list_memories", { kind, query }),
  addMemory: (kind: MemoryKind, content: string) =>
    call<Memory>("add_memory", { kind, content }),
  updateMemory: (id: number, content: string, kind?: MemoryKind) =>
    call<Memory>("update_memory", { id, content, kind }),
  deleteMemory: (id: number) => call<void>("delete_memory", { id }),
  getVoiceStatus: () => call<VoiceStatus>("get_voice_status"),
  transcribeAudio: (audioBase64: string, mimeType: string) =>
    call<string>("transcribe_audio", { audioBase64, mimeType }),
  synthesizeSpeech: (text: string) =>
    call<string>("synthesize_speech", { text }),
  listFolders: () => call<AllowedFolder[]>("list_folders"),
  addFolder: (path: string, writable: boolean) =>
    call<AllowedFolder>("add_folder", { path, writable }),
  setFolderWritable: (id: number, writable: boolean) =>
    call<void>("set_folder_writable", { id, writable }),
  removeFolder: (id: number) => call<void>("remove_folder", { id }),
  suggestFolders: () => call<FolderSuggestion[]>("suggest_folders"),
  listProjects: () => call<Project[]>("list_projects"),
  addProject: (project: ProjectInput) =>
    call<Project>("add_project", { project }),
  updateProject: (id: number, project: ProjectInput) =>
    call<Project>("update_project", { id, project }),
  removeProject: (id: number) => call<void>("remove_project", { id }),
  detectProject: (path: string) =>
    call<DetectedProject>("detect_project", { path }),
  listTasks: (filter: "open" | "done" | "all") =>
    call<Task[]>("list_tasks", { filter }),
  addTask: (task: TaskInput) => call<Task>("add_task", { task }),
  updateTask: (id: number, task: TaskInput) =>
    call<Task>("update_task", { id, task }),
  setTaskDone: (id: number, done: boolean) =>
    call<Task>("set_task_done", { id, done }),
  deleteTask: (id: number) => call<void>("delete_task", { id }),
  clearDoneTasks: () => call<number>("clear_done_tasks"),
  parseTime: (text: string) => call<ParsedTime>("parse_time", { text }),
  listReminders: () => call<Reminder[]>("list_reminders"),
  reminderHistory: () => call<Reminder[]>("reminder_history"),
  addReminder: (title: string, when: string) =>
    call<Reminder>("add_reminder", { title, when }),
  startTimer: (label: string, duration: string) =>
    call<Reminder>("start_timer", { label, duration }),
  cancelReminder: (id: number) => call<Reminder>("cancel_reminder", { id }),
  dismissReminder: (id: number) => call<Reminder>("dismiss_reminder", { id }),
  snoozeReminder: (id: number, minutes: number) =>
    call<Reminder>("snooze_reminder", { id, minutes }),
  clearReminderHistory: () => call<number>("clear_reminder_history"),
  listEvents: (from: string, to: string) =>
    call<CalendarEvent[]>("list_events", { from, to }),
  addEvent: (event: EventInput) => call<CalendarEvent>("add_event", { event }),
  updateEvent: (id: number, event: EventInput) =>
    call<CalendarEvent>("update_event", { id, event }),
  deleteEvent: (id: number) => call<void>("delete_event", { id }),
  devicesOverview: () => call<DevicesOverview>("devices_overview"),
  homeDevices: () => call<HomeDevice[]>("home_devices"),
  homeDiscover: () => call<FoundDevice[]>("home_discover"),
  homePair: (host: string, name?: string) =>
    call<HomeDevice>("home_pair", { host, name }),
  homeRemove: (id: string) => call<void>("home_remove", { id }),
  homeRename: (id: string, name: string) =>
    call<void>("home_rename", { id, name }),
  homeTv: (id: string, action: TvActionName, value?: string | number) =>
    call<string>("home_tv", { id, action, value }),
  homeTvState: (id: string) => call<TvState>("home_tv_state", { id }),
  homePointer: (id: string, events: TvPointerEvent[]) =>
    call<void>("home_pointer", { id, events }),
  homePointerStop: (id: string) => call<void>("home_pointer_stop", { id }),
  devicesConfigure: (
    relayUrl: string | null,
    enabled: boolean,
    syncMemory: boolean,
  ) =>
    call<DeviceSettings>("devices_configure", {
      relayUrl,
      enabled,
      syncMemory,
    }),
  devicesStartPairing: () => call<PairingCode>("devices_start_pairing"),
  devicesCancelPairing: () => call<void>("devices_cancel_pairing"),
  devicesConfirmPairing: (pairingId: string, allow: boolean) =>
    call<PairedDevice | null>("devices_confirm_pairing", { pairingId, allow }),
  devicesJoin: (code: string) => call<PairedDevice>("devices_join", { code }),
  devicesRevoke: (deviceId: string) =>
    call<void>("devices_revoke", { deviceId }),
  devicesForget: (deviceId: string) =>
    call<void>("devices_forget", { deviceId }),
  devicesRename: (deviceId: string, name: string) =>
    call<PairedDevice>("devices_rename", { deviceId, name }),
  devicesRenameThis: (name: string) =>
    call<ThisDevice>("devices_rename_this", { name }),
  devicesHandoff: (deviceId: string, kind: "link" | "text", content: string) =>
    call<boolean>("devices_handoff", { deviceId, kind, content }),
  handoffsList: () => call<Handoff[]>("handoffs_list"),
  handoffDismiss: (id: string) => call<void>("handoff_dismiss", { id }),
  devicesSendTask: (deviceId: string, objective: string) =>
    call<RemoteTask>("devices_send_task", { deviceId, objective }),
  devicesControlTask: (requestId: string, action: TaskControl) =>
    call<RemoteTask>("devices_control_task", { requestId, action }),
  devicesAnswerApproval: (callId: string, approve: boolean) =>
    call<void>("devices_answer_approval", { callId, approve }),
  devicesAudit: (limit?: number) =>
    call<DeviceAuditRow[]>("devices_audit", { limit }),
};

/** A file's bytes as base64 (without the data-URL prefix). */
function fileToBase64(file: File): Promise<string> {
  return new Promise((resolve, reject) => {
    const r = new FileReader();
    r.onload = () => {
      const url = String(r.result);
      resolve(url.slice(url.indexOf(",") + 1));
    };
    r.onerror = () => reject(new Error("Couldn't read that file."));
    r.readAsDataURL(file);
  });
}

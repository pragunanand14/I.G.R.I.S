import type { AiStatus } from "@/types/ai";
import type { Attachment, AttachmentData } from "@/types/attachments";
import type { AppInfo, PublicConfig, ReloadResult } from "@/types/app";
import type { Conversation, ConversationDetail } from "@/types/chat";
import type { Memory, MemoryKind } from "@/types/memory";
import type { CalendarEvent, EventInput, ParsedTime, Reminder, Task, TaskInput } from "@/types/productivity";
import type { VoiceStatus } from "@/types/voice";
import type { AllowedFolder, DetectedProject, FolderSuggestion, Project, ProjectInput } from "@/types/workspace";
import type { Settings, SettingsPatch } from "@/types/settings";
import type { SystemSnapshot } from "@/types/system";
import type { AppCandidate, AppEntry, AuditEntry, PermissionRule, ToolActivity, ToolInfo } from "@/types/tools";
import { call, callRaw } from "./backend";
import type { OperatorAction, OperatorSnapshot, OperatorTask } from "@/types/operator";
import type { TaskControl, TaskInfo } from "@/types/task";

/** All IPC commands in one place — mirrors `generate_handler!` in `src-tauri/src/lib.rs`. */
export const api = {
  getAppInfo: () => call<AppInfo>("get_app_info"),
  getConfigStatus: () => call<PublicConfig>("get_config_status"),
  getSettings: () => call<Settings>("get_settings"),
  updateSettings: (patch: SettingsPatch) => call<Settings>("update_settings", { patch }),
  getSystemSnapshot: () => call<SystemSnapshot>("get_system_snapshot"),
  getAiStatus: () => call<AiStatus>("get_ai_status"),
  reloadConfig: () => call<ReloadResult>("reload_config"),
  listConversations: () => call<Conversation[]>("list_conversations"),
  getConversation: (id: string) => call<ConversationDetail>("get_conversation", { id }),
  renameConversation: (id: string, title: string) => call<Conversation>("rename_conversation", { id, title }),
  deleteConversation: (id: string) => call<void>("delete_conversation", { id }),
  cancelChat: (requestId: string) => call<boolean>("chat_cancel", { requestId }),
  attachFile: async (file: File) =>
    callRaw<Attachment>("attach_file", new Uint8Array(await file.arrayBuffer()), { "x-file-name": encodeURIComponent(file.name) }),
  discardAttachment: (id: string) => call<void>("discard_attachment", { id }),
  readAttachment: (id: string) => call<AttachmentData>("read_attachment", { id }),
  getOperatorState: () => call<OperatorSnapshot>("get_operator_state"),
  operatorControl: (action: OperatorAction) => call<OperatorSnapshot>("operator_control", { action }),
  listOperatorTasks: (limit?: number) => call<OperatorTask[]>("list_operator_tasks", { limit }),
  listConversationTasks: (conversationId: string, limit?: number) => call<TaskInfo[]>("list_conversation_tasks", { conversationId, limit }),
  taskControl: (taskId: string, action: TaskControl) => call<TaskInfo>("task_control", { taskId, action }),
  trustConversation: (conversationId: string) => call<void>("trust_conversation", { conversationId }),
  respondToolApproval: (callId: string, approved: boolean) => call<boolean>("respond_tool_approval", { callId, approved }),
  listTools: () => call<ToolInfo[]>("list_tools"),
  listToolAudit: (limit?: number) => call<AuditEntry[]>("list_tool_audit", { limit }),
  getPermissionPolicy: () => call<PermissionRule[]>("get_permission_policy"),
  listApplications: () => call<AppEntry[]>("list_applications"),
  addApplication: (name: string, path: string) => call<AppEntry>("add_application", { name, path }),
  removeApplication: (id: string) => call<void>("remove_application", { id }),
  detectApplications: () => call<AppCandidate[]>("detect_applications"),
  launchApplication: (id: string) => call<ToolActivity>("launch_application", { id }),
  listMemories: (kind?: MemoryKind, query?: string) => call<Memory[]>("list_memories", { kind, query }),
  addMemory: (kind: MemoryKind, content: string) => call<Memory>("add_memory", { kind, content }),
  updateMemory: (id: number, content: string, kind?: MemoryKind) => call<Memory>("update_memory", { id, content, kind }),
  deleteMemory: (id: number) => call<void>("delete_memory", { id }),
  getVoiceStatus: () => call<VoiceStatus>("get_voice_status"),
  transcribeAudio: (audioBase64: string, mimeType: string) => call<string>("transcribe_audio", { audioBase64, mimeType }),
  synthesizeSpeech: (text: string) => call<string>("synthesize_speech", { text }),
  listFolders: () => call<AllowedFolder[]>("list_folders"),
  addFolder: (path: string, writable: boolean) => call<AllowedFolder>("add_folder", { path, writable }),
  setFolderWritable: (id: number, writable: boolean) => call<void>("set_folder_writable", { id, writable }),
  removeFolder: (id: number) => call<void>("remove_folder", { id }),
  suggestFolders: () => call<FolderSuggestion[]>("suggest_folders"),
  listProjects: () => call<Project[]>("list_projects"),
  addProject: (project: ProjectInput) => call<Project>("add_project", { project }),
  updateProject: (id: number, project: ProjectInput) => call<Project>("update_project", { id, project }),
  removeProject: (id: number) => call<void>("remove_project", { id }),
  detectProject: (path: string) => call<DetectedProject>("detect_project", { path }),
  listTasks: (filter: "open" | "done" | "all") => call<Task[]>("list_tasks", { filter }),
  addTask: (task: TaskInput) => call<Task>("add_task", { task }),
  updateTask: (id: number, task: TaskInput) => call<Task>("update_task", { id, task }),
  setTaskDone: (id: number, done: boolean) => call<Task>("set_task_done", { id, done }),
  deleteTask: (id: number) => call<void>("delete_task", { id }),
  clearDoneTasks: () => call<number>("clear_done_tasks"),
  parseTime: (text: string) => call<ParsedTime>("parse_time", { text }),
  listReminders: () => call<Reminder[]>("list_reminders"),
  reminderHistory: () => call<Reminder[]>("reminder_history"),
  addReminder: (title: string, when: string) => call<Reminder>("add_reminder", { title, when }),
  startTimer: (label: string, duration: string) => call<Reminder>("start_timer", { label, duration }),
  cancelReminder: (id: number) => call<Reminder>("cancel_reminder", { id }),
  dismissReminder: (id: number) => call<Reminder>("dismiss_reminder", { id }),
  snoozeReminder: (id: number, minutes: number) => call<Reminder>("snooze_reminder", { id, minutes }),
  clearReminderHistory: () => call<number>("clear_reminder_history"),
  listEvents: (from: string, to: string) => call<CalendarEvent[]>("list_events", { from, to }),
  addEvent: (event: EventInput) => call<CalendarEvent>("add_event", { event }),
  updateEvent: (id: number, event: EventInput) => call<CalendarEvent>("update_event", { id, event }),
  deleteEvent: (id: number) => call<void>("delete_event", { id }),
};

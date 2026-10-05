import {
  Brain,
  CheckSquare,
  Cpu,
  FolderGit2,
  Home,
  MessageSquare,
  Settings,
  ShieldCheck,
  Wrench,
  type LucideIcon,
} from "lucide-react";

export interface NavItem {
  path: string;
  label: string;
  icon: LucideIcon;
  /** `undefined` = implemented. Otherwise the roadmap phase that delivers it. */
  plannedPhase?: number;
  summary: string;
  planned?: string[];
}

export const NAV_ITEMS: NavItem[] = [
  { path: "/", label: "Home", icon: Home, summary: "IGRIS core and live system status." },
  {
    path: "/chat",
    label: "Chat",
    icon: MessageSquare,
    summary: "Streaming conversations with IGRIS.",
  },
  {
    path: "/tasks",
    label: "Tasks",
    icon: CheckSquare,
    plannedPhase: 8,
    summary: "Tasks, reminders and notifications.",
    planned: ["Natural-language reminders", "Native notifications", "Calendar integration"],
  },
  {
    path: "/memory",
    label: "Memory",
    icon: Brain,
    summary: "What IGRIS remembers — inspectable, editable, deletable.",
  },
  {
    path: "/projects",
    label: "Projects",
    icon: FolderGit2,
    plannedPhase: 7,
    summary: "Project context for developer mode.",
    planned: ["Project registry (path, repo, stack)", "Load project context on request", "Developer tools: search, tests, git"],
  },
  { path: "/system", label: "System", icon: Cpu, summary: "Live hardware and OS telemetry." },
  {
    path: "/tools",
    label: "Tools",
    icon: Wrench,
    summary: "The controlled tool layer IGRIS uses to act.",
  },
  {
    path: "/security",
    label: "Security",
    icon: ShieldCheck,
    summary: "Permissions and audit trail.",
  },
  { path: "/settings", label: "Settings", icon: Settings, summary: "Preferences and configuration." },
];

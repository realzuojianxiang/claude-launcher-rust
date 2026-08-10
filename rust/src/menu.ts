// 左侧导航菜单定义：后台管理式菜单项，可折叠收起。
// MenuKey 同时被 App 用于 useState<MenuKey> 联合字面量类型；MenuItem 用于 active 项断言。
// icon 直接使用 lucide-react 图标组件，统一风格、矢量清晰、支持 currentColor 着色。

import {
  LayoutDashboard,
  Rocket,
  Cpu,
  Network,
  ScrollText,
  Settings,
  Info,
  BookOpen,
  Webhook,
  type LucideIcon,
} from "lucide-react";

export type MenuKey =
  | "dashboard"
  | "launch"
  | "nvidia"
  | "gateway"
  | "openai_gw"
  | "logs"
  | "config"
  | "dictionary"
  | "about";

export interface MenuItem {
  key: MenuKey;
  label: string;
  icon: LucideIcon;
}

// 顺序：仪表盘 → 启动 Claude → NVIDIA 代理 → 协议网关 → OpenAI 透传网关 → 日志 → 配置 → 单词本 → 关于
export const MENU: MenuItem[] = [
  { key: "dashboard", label: "仪表盘", icon: LayoutDashboard },
  { key: "launch", label: "启动 Claude", icon: Rocket },
  { key: "nvidia", label: "NVIDIA 代理", icon: Cpu },
  { key: "gateway", label: "协议网关", icon: Network },
  { key: "openai_gw", label: "OpenAI 网关", icon: Webhook },
  { key: "logs", label: "日志", icon: ScrollText },
  { key: "config", label: "配置", icon: Settings },
  { key: "dictionary", label: "单词本", icon: BookOpen },
  { key: "about", label: "关于", icon: Info },
];

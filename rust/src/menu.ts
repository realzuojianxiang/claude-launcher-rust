// 左侧导航菜单定义：后台管理式菜单项，可折叠收起。
// MenuKey 同时被 App 用于 useState<MenuKey> 联合字面量类型；MenuItem 用于 active 项断言。
// icon 直接使用 lucide-react 图标组件，统一风格、矢量清晰、支持 currentColor 着色。

import {
  LayoutDashboard,
  Rocket,
  Network,
  Cpu,
  ScrollText,
  Settings,
  Info,
  BookOpen,
  type LucideIcon,
} from "lucide-react";

export type MenuKey =
  | "dashboard"
  | "launch"
  | "proxy"
  | "nvidia"
  | "logs"
  | "config"
  | "dictionary"
  | "about";

export interface MenuItem {
  key: MenuKey;
  label: string;
  icon: LucideIcon;
}

// 顺序：仪表盘 → 启动 Claude → CLIProxyAPI → NVIDIA 代理 → 日志 → 配置 → 单词本 → 关于
export const MENU: MenuItem[] = [
  { key: "dashboard", label: "仪表盘", icon: LayoutDashboard },
  { key: "launch", label: "启动 Claude", icon: Rocket },
  { key: "proxy", label: "CLIProxyAPI", icon: Network },
  { key: "nvidia", label: "NVIDIA 代理", icon: Cpu },
  { key: "logs", label: "日志", icon: ScrollText },
  { key: "config", label: "配置", icon: Settings },
  { key: "dictionary", label: "单词本", icon: BookOpen },
  { key: "about", label: "关于", icon: Info },
];

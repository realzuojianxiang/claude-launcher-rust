import { type ComponentType } from "react";
import {
  PanelLeftClose,
  PanelLeftOpen,
  type LucideProps,
} from "lucide-react";

export interface SidebarItem {
  key: string;
  label: string;
  icon: ComponentType<LucideProps>;
}

export interface SidebarProps {
  /** 菜单项列表（按显示顺序） */
  items: SidebarItem[];
  /** 当前激活项的 key（受控） */
  activeKey: string;
  /** 选中菜单项回调（受控） */
  onSelect: (key: string) => void;
  /** 是否折叠（受控） */
  collapsed: boolean;
  /** 切换折叠状态回调（受控） */
  onToggleCollapse: () => void;
  /** 左上角品牌名 */
  brand?: string;
  /** 左上角品牌图标 */
  brandIcon?: ComponentType<LucideProps>;
  /** 底部版本信息（折叠时隐藏） */
  version?: string;
  className?: string;
}

/**
 * 轻量 Apple 风格侧边栏：
 * - 受控折叠状态（collapsed / onToggleCollapse）
 * - 折叠后仅显示圆角图标，展开后显示「图标 + 文字」
 * - 每个菜单项左侧为浅色圆角图标容器
 * - 激活项背景 + 文字/图标高亮；hover 与按下均有反馈（轻微按压缩放）
 */
export default function Sidebar({
  items,
  activeKey,
  onSelect,
  collapsed,
  onToggleCollapse,
  brand = "App",
  brandIcon: BrandIcon,
  version,
  className = "",
}: SidebarProps) {
  return (
    <aside
      className={`flex h-full flex-col border-r border-black/[0.06] bg-white/70 backdrop-blur-xl transition-[width] duration-300 ease-[cubic-bezier(0.32,0.72,0,1)] ${
        collapsed ? "w-[76px]" : "w-64"
      } ${className}`}
    >
      {/* 品牌区 */}
      <div
        className={`flex h-[60px] shrink-0 items-center border-b border-black/[0.06] ${
          collapsed ? "justify-center px-0" : "gap-3 px-5"
        }`}
      >
        <span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-xl bg-gradient-to-br from-blue-500 to-indigo-500 text-white shadow-sm">
          {BrandIcon ? <BrandIcon size={18} strokeWidth={2.2} /> : null}
        </span>
        {!collapsed && (
          <span className="truncate text-[15px] font-bold tracking-tight text-slate-800">
            {brand}
          </span>
        )}
      </div>

      {/* 菜单 */}
      <nav className="flex-1 space-y-1 overflow-y-auto px-3 py-4">
        {items.map((item) => {
          const Icon = item.icon;
          const isActive = item.key === activeKey;
          return (
            <button
              key={item.key}
              type="button"
              onClick={() => onSelect(item.key)}
              title={collapsed ? item.label : undefined}
              aria-current={isActive ? "page" : undefined}
              className={`group flex w-full items-center rounded-xl py-2.5 text-left transition-all duration-200 ease-out hover:bg-black/[0.04] active:scale-[0.97] ${
                collapsed ? "justify-center px-0" : "gap-3 px-2.5"
              } ${isActive ? "bg-blue-50" : ""}`}
            >
              {/* 浅色圆角图标容器 */}
              <span
                className={`flex h-9 w-9 shrink-0 items-center justify-center rounded-xl transition-colors duration-200 ${
                  isActive
                    ? "bg-blue-500 text-white shadow-sm"
                    : "bg-slate-100 text-slate-500 group-hover:bg-slate-200/80 group-hover:text-slate-700"
                }`}
              >
                <Icon size={18} strokeWidth={2} />
              </span>
              {!collapsed && (
                <span
                  className={`truncate text-[14px] tracking-tight transition-colors duration-200 ${
                    isActive
                      ? "font-semibold text-blue-600"
                      : "font-medium text-slate-600 group-hover:text-slate-900"
                  }`}
                >
                  {item.label}
                </span>
              )}
            </button>
          );
        })}
      </nav>

      {/* 底部：折叠按钮 + 版本 */}
      <div className="shrink-0 border-t border-black/[0.06] p-3">
        <button
          type="button"
          onClick={onToggleCollapse}
          title={collapsed ? "展开菜单" : "折叠菜单"}
          aria-label={collapsed ? "展开菜单" : "折叠菜单"}
          className={`group flex w-full items-center rounded-xl py-2 text-slate-500 transition-all duration-200 ease-out hover:bg-black/[0.04] active:scale-[0.97] ${
            collapsed ? "justify-center px-0" : "gap-3 px-2.5"
          }`}
        >
          <span className="flex h-9 w-9 shrink-0 items-center justify-center rounded-xl bg-slate-100 text-slate-500 transition-colors duration-200 group-hover:bg-slate-200/80 group-hover:text-slate-700">
            {collapsed ? (
              <PanelLeftOpen size={18} strokeWidth={2} />
            ) : (
              <PanelLeftClose size={18} strokeWidth={2} />
            )}
          </span>
          {!collapsed && (
            <span className="truncate text-[13px] font-medium tracking-tight text-slate-500">
              收起侧栏
            </span>
          )}
        </button>
        {!collapsed && version && (
          <p className="mt-1 text-center text-[11px] tracking-wide text-slate-400">
            {version}
          </p>
        )}
      </div>
    </aside>
  );
}

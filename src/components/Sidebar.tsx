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
      className={`sidebar ${collapsed ? "sidebar--collapsed" : ""} ${className}`.trim()}
    >
      {/* 品牌区 */}
      <div className="sidebar__brand">
        <span className="sidebar__brand-mark">
          {BrandIcon ? <BrandIcon size={18} strokeWidth={2.2} aria-hidden="true" /> : null}
        </span>
        {!collapsed && <span className="sidebar__brand-name">{brand}</span>}
      </div>

      {/* 菜单 */}
      <nav aria-label="主导航" className="sidebar__nav">
        {items.map((item) => {
          const Icon = item.icon;
          const isActive = item.key === activeKey;
          return (
            <button
              key={item.key}
              type="button"
              onClick={() => onSelect(item.key)}
              aria-label={collapsed ? item.label : undefined}
              aria-current={isActive ? "page" : undefined}
              className={`sidebar__item ${isActive ? "sidebar__item--active" : ""}`.trim()}
            >
              <span className="sidebar__icon">
                <Icon size={18} strokeWidth={2} aria-hidden="true" />
              </span>
              {!collapsed && (
                <span className="sidebar__label">{item.label}</span>
              )}
            </button>
          );
        })}
      </nav>

      {/* 底部：折叠按钮 + 版本 */}
      <div className="sidebar__footer">
        <button
          type="button"
          onClick={onToggleCollapse}
          aria-label={collapsed ? "展开菜单" : "折叠菜单"}
          className="sidebar__toggle"
        >
          <span className="sidebar__icon sidebar__icon--toggle">
            {collapsed ? (
              <PanelLeftOpen size={18} strokeWidth={2} aria-hidden="true" />
            ) : (
              <PanelLeftClose size={18} strokeWidth={2} aria-hidden="true" />
            )}
          </span>
          {!collapsed && <span className="sidebar__toggle-label">收起侧栏</span>}
        </button>
        {!collapsed && version && (
          <p className="sidebar__version">{version}</p>
        )}
      </div>
    </aside>
  );
}
